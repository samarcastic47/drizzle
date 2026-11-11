//! Drizzle ($DRIZZLE): a NEP-141 token that mints only when it rains.
//!
//! Each round, anyone calls `tick()`: the contract asks OutLayer to run the pinned
//! weather oracle WASM, then `on_weather` settles all 12 city pools with the
//! returned precipitation. The pool math lives in [`pool`]; this file wires it to
//! the token, storage and the oracle.

pub mod config;
pub mod events;
pub mod oracle;
pub mod pool;
pub mod user;
pub mod views;

use crate::config::{Config, NUM_CITIES};
use crate::events::{
    DrizzleEvents, PlantEvent, RainDanceEvent, RoundEvent, RoundFailedEvent, SeedEvent, UprootEvent,
};
use crate::oracle::{ext_self, IN_FLIGHT_TIMEOUT_SECS};
use crate::user::{CityState, UserCity};
use crate::views::{
    CityView, InFlightView, RoundLog, RoundStateView, StatsView, UserCityView, UserView,
};
use near_contract_standards::fungible_token::core::FungibleTokenCore;
use near_contract_standards::fungible_token::events::{FtBurn, FtMint};
use near_contract_standards::fungible_token::metadata::FungibleTokenMetadata;
use near_contract_standards::fungible_token::resolver::FungibleTokenResolver;
use near_contract_standards::fungible_token::FungibleToken;
use near_contract_standards::storage_management::{
    StorageBalance, StorageBalanceBounds, StorageManagement,
};
use near_sdk::collections::{LookupMap, LookupSet};
use near_sdk::json_types::U128;
use near_sdk::serde_json::Value;
use near_sdk::{
    assert_one_yocto, env, log, near, require, AccountId, NearToken, PanicOnDefault, Promise,
    PromiseError, PromiseOrValue,
};

/// Hard cap on `get_round_logs` (interface).
pub const MAX_ROUND_LOG_PAGE: u32 = 100;
/// Live rounds are 3600 s; the data-availability grace after the hour closes.
pub const LIVE_GRACE_SECS: u64 = 120;

/// The one OutLayer request currently in flight.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug)]
pub struct InFlight {
    pub round: u64,
    pub started_ts: u64,
    /// Block of the `tick()` tx, echoed into the `RoundLog`.
    pub tick_block_height: u64,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Contract {
    pub cfg: Config,
    pub ft: FungibleToken,

    /// 12 pools, indexed like `cfg.cities`.
    pub cities: Vec<CityState>,
    /// `(account, city) -> position`, created lazily.
    pub users: LookupMap<(AccountId, u8), UserCity>,
    pub seeded: LookupSet<AccountId>,
    pub round_logs: LookupMap<u64, RoundLog>,
    /// `(round, city) -> (pending_in, shares_out)` for rounds that sprouted.
    pub sprout_ratio: LookupMap<(u64, u8), (u128, u128)>,

    pub next_round: u64,
    pub in_flight: Option<InFlight>,

    pub minted: u128,
    pub burned: u128,
    pub danced: u128,
    pub seeded_count: u64,
    pub rounds_settled: u64,

    /// Bytes occupied by one account's 12 `UserCity` entries (measured in `new`).
    pub user_extra_storage: u64,
}

#[near]
impl Contract {
    /// Initializes the game. Immutable config; the account itself must call this.
    #[init]
    #[private]
    pub fn new(config: Config) -> Self {
        config.validate();
        let mut ft = FungibleToken::new(b"ft".to_vec());
        // The contract itself holds pool value + pending, so it needs a balance.
        ft.internal_register_account(&env::current_account_id());

        let mut users: LookupMap<(AccountId, u8), UserCity> = LookupMap::new(b"us".to_vec());
        // Measure the storage one account's 12 lazily-created entries occupy, so
        // `storage_balance_bounds().min` covers a full position set.
        let probe: AccountId = "a".repeat(64).parse().unwrap();
        let before = env::storage_usage();
        for city in 0..NUM_CITIES {
            users.insert(&(probe.clone(), city as u8), &UserCity::default());
        }
        let user_extra_storage = env::storage_usage() - before;
        for city in 0..NUM_CITIES {
            users.remove(&(probe.clone(), city as u8));
        }

        Self {
            ft,
            cities: vec![CityState::default(); NUM_CITIES],
            users,
            seeded: LookupSet::new(b"s".to_vec()),
            round_logs: LookupMap::new(b"rl".to_vec()),
            sprout_ratio: LookupMap::new(b"sr".to_vec()),
            next_round: 0,
            in_flight: None,
            minted: 0,
            burned: 0,
            danced: 0,
            seeded_count: 0,
            rounds_settled: 0,
            user_extra_storage,
            cfg: config,
        }
    }

    // ---------------------------------------------------------------- user side

    /// Registers storage if needed and mints `seed_amount`, once per account.
    #[payable]
    pub fn claim_seed(&mut self) {
        let account_id = env::predecessor_account_id();
        self.deposit_registration(&account_id);
        require!(!self.seeded.contains(&account_id), "already seeded");

        self.seeded.insert(&account_id);
        self.seeded_count += 1;
        let amount = self.cfg.seed_amount.0;
        if amount > 0 {
            self.ft.internal_deposit(&account_id, amount);
            self.minted += amount;
            FtMint {
                owner_id: &account_id,
                amount: U128(amount),
                memo: Some("seed"),
            }
            .emit();
        }
        DrizzleEvents::Seed(vec![SeedEvent {
            account_id,
            amount: self.cfg.seed_amount,
        }])
        .emit();
    }

    /// Moves `amount` from the caller's balance into the city's pending pool. The
    /// tokens sprout into shares after round `next_round` settles.
    #[payable]
    pub fn plant(&mut self, city: u8, amount: U128) {
        assert_one_yocto();
        self.assert_city(city);
        let amount = amount.0;
        require!(amount > 0, "amount must be positive");

        let account_id = env::predecessor_account_id();
        let mut position = self.users.get(&(account_id.clone(), city)).unwrap_or_default();
        self.sprout_position(&mut position, city);

        let was_planted = position.shares > 0 || position.pending > 0;
        self.ft.internal_transfer(
            &account_id,
            &env::current_account_id(),
            amount,
            Some("plant".to_string()),
        );

        let mut pool = self.cities[city as usize].to_pool();
        pool::plant(&mut pool, amount);
        let state = &mut self.cities[city as usize];
        state.absorb(&pool);
        if !was_planted {
            state.planters += 1;
        }

        if position.pending == 0 {
            position.pending_round = self.next_round;
        }
        position.pending += amount;
        self.users.insert(&(account_id.clone(), city), &position);

        DrizzleEvents::Plant(vec![PlantEvent {
            account_id,
            city,
            amount: U128(amount),
            sprouts_after_round: self.next_round,
        }])
        .emit();
    }

    /// Burns `shares` (all of them when `None`) and returns unsprouted pending 1:1.
    /// When the caller is the city's last planter, the whole remaining pool value
    /// (sprout dust included) is swept to them.
    #[payable]
    pub fn uproot(&mut self, city: u8, shares: Option<U128>) -> U128 {
        assert_one_yocto();
        self.assert_city(city);
        let account_id = env::predecessor_account_id();
        let key = (account_id.clone(), city);
        let mut position = self.users.get(&key).unwrap_or_default();
        self.sprout_position(&mut position, city);

        let was_planted = position.shares > 0 || position.pending > 0;
        let requested = match shares {
            Some(shares) => u128::from(shares),
            None => position.shares,
        };
        require!(requested <= position.shares, "not enough shares");

        let mut pool = self.cities[city as usize].to_pool();
        let mut credit = pool::uproot(&mut pool, requested);
        position.shares -= requested;

        if shares.is_none() && position.pending > 0 {
            let returned_pending = position.pending;
            pool::unplant_pending(&mut pool, returned_pending);
            position.pending = 0;
            position.pending_round = 0;
            credit += returned_pending;
        }

        let state = &mut self.cities[city as usize];
        let now_planted = position.shares > 0 || position.pending > 0;
        if was_planted && !now_planted {
            state.planters = state.planters.saturating_sub(1);
        }
        if state.planters == 0 {
            // Last-holder sweep: nobody can earn from this pool any more.
            credit += pool.value;
            pool.value = 0;
            pool.shares = 0;
        }
        state.absorb(&pool);

        if position.is_empty() {
            self.users.remove(&key);
        } else {
            self.users.insert(&key, &position);
        }
        self.credit(&account_id, credit);

        DrizzleEvents::Uproot(vec![UprootEvent {
            account_id,
            city,
            shares: U128(requested),
            amount: U128(credit),
        }])
        .emit();
        U128(credit)
    }

    /// Burns `amount` from the caller's balance into the city's jackpot.
    #[payable]
    pub fn rain_dance(&mut self, city: u8, amount: U128) {
        assert_one_yocto();
        self.assert_city(city);
        let amount = amount.0;
        require!(amount > 0, "amount must be positive");

        let account_id = env::predecessor_account_id();
        self.ft.internal_withdraw(&account_id, amount);
        self.burned += amount;
        self.danced += amount;
        FtBurn {
            owner_id: &account_id,
            amount: U128(amount),
            memo: Some("rain_dance"),
        }
        .emit();

        let mut pool = self.cities[city as usize].to_pool();
        pool::rain_dance(&mut pool, amount);
        let state = &mut self.cities[city as usize];
        state.absorb(&pool);

        DrizzleEvents::RainDance(vec![RainDanceEvent {
            account_id,
            city,
            amount: U128(amount),
            jackpot: U128(state.jackpot),
        }])
        .emit();
    }

    // ---------------------------------------------------------------- rounds

    /// Requests the weather for `next_round` from OutLayer. Anyone may call this;
    /// the caller (keeper) pays the oracle deposit and receives the tip on success.
    #[payable]
    pub fn tick(&mut self) -> Promise {
        let attached = env::attached_deposit();
        let oracle_deposit = NearToken::from_yoctonear(self.cfg.oracle.deposit.0);
        require!(
            attached >= oracle_deposit,
            "attached deposit is less than oracle.deposit"
        );

        let now = env::block_timestamp() / 1_000_000_000;
        let round = self.next_round;
        require!(now >= self.due_at(round), "round not due");
        if let Some(in_flight) = &self.in_flight {
            require!(
                now >= in_flight.started_ts + IN_FLIGHT_TIMEOUT_SECS,
                "tick in flight"
            );
        }

        let keeper = env::predecessor_account_id();
        self.in_flight = Some(InFlight {
            round,
            started_ts: now,
            tick_block_height: env::block_height(),
        });

        // Overpayment is returned to the keeper immediately; OutLayer refunds the
        // unused part of `oracle_deposit` to `payer_account_id` (= the keeper).
        let refund = attached.saturating_sub(oracle_deposit);
        if refund > NearToken::from_yoctonear(0) {
            Promise::new(keeper.clone()).transfer(refund).detach();
        }

        let input = oracle::build_input(self.cfg.first_hour + round, &self.cfg.cities);
        oracle::request_promise(
            self.cfg.oracle.outlayer.clone(),
            &self.cfg.oracle.source,
            &self.cfg.oracle.resource_limits,
            input,
            keeper.clone(),
            oracle_deposit,
        )
        .then(
            ext_self::ext(env::current_account_id())
                .with_static_gas(oracle::WEATHER_CALLBACK_GAS)
                .on_weather(round, keeper),
        )
    }

    /// OutLayer settlement callback. Only reachable through `tick()`'s promise.
    #[private]
    pub fn on_weather(
        &mut self,
        round: u64,
        keeper: AccountId,
        #[callback_result] result: Result<Option<Value>, PromiseError>,
    ) {
        // The `#[private]` wrapper already checks this on chain; repeating it keeps
        // the guarantee testable off-chain (the generated wrapper only exists on
        // wasm32).
        if env::current_account_id() != env::predecessor_account_id() {
            env::panic_str("Method on_weather is private");
        }

        let Some(in_flight) = self.in_flight.clone() else {
            log!("on_weather: no request in flight, ignoring round {}", round);
            return;
        };
        if in_flight.round != round {
            log!(
                "on_weather: stale callback for round {} (in flight: {}), ignoring",
                round,
                in_flight.round
            );
            return;
        }
        if round != self.next_round {
            log!(
                "on_weather: round {} is not the next round ({}), ignoring",
                round,
                self.next_round
            );
            return;
        }

        let expected_hour = self.cfg.first_hour + round;
        let payload = match result {
            Ok(Some(value)) => value,
            Ok(None) => return self.fail_round(round, "outlayer returned null"),
            Err(_) => return self.fail_round(round, "outlayer call failed or never resumed"),
        };
        let reading = match oracle::parse_reading(payload, expected_hour, NUM_CITIES) {
            Ok(reading) => reading,
            Err(reason) => return self.fail_round(round, &reason),
        };

        let params = self.cfg.pool_params();
        let mut pools: Vec<pool::Pool> = self.cities.iter().map(CityState::to_pool).collect();
        let jackpots: Vec<u128> = pools.iter().map(|p| p.jackpot).collect();
        let outcomes = pool::settle_round(&mut pools, &reading.mm_tenths, round, &params);

        let contract_id = env::current_account_id();
        let mut minted_log = Vec::with_capacity(NUM_CITIES);
        let mut burned_log = Vec::with_capacity(NUM_CITIES);
        let mut jackpot_log = Vec::with_capacity(NUM_CITIES);
        let mut sprouted_log = Vec::with_capacity(NUM_CITIES);

        for (city, outcome) in outcomes.iter().enumerate() {
            if outcome.minted > 0 {
                self.ft.internal_deposit(&contract_id, outcome.minted);
                self.minted += outcome.minted;
                FtMint {
                    owner_id: &contract_id,
                    amount: U128(outcome.minted),
                    memo: Some("rain"),
                }
                .emit();
            }
            if outcome.burned > 0 {
                self.ft.internal_withdraw(&contract_id, outcome.burned);
                self.burned += outcome.burned;
                FtBurn {
                    owner_id: &contract_id,
                    amount: U128(outcome.burned),
                    memo: Some("evaporation"),
                }
                .emit();
            }
            if let Some(ratio) = outcome.sprout {
                self.sprout_ratio.insert(&(round, city as u8), &ratio);
            }
            minted_log.push(U128(outcome.minted));
            burned_log.push(U128(outcome.burned));
            jackpot_log.push(U128(if outcome.minted > 0 {
                jackpots[city]
            } else {
                0
            }));
            sprouted_log.push(U128(outcome.sprout.map(|(pending, _)| pending).unwrap_or(0)));
        }

        for (city, pool) in pools.iter().enumerate() {
            self.cities[city].absorb(pool);
        }

        // Keeper tip: minted from nothing, so skip it when the keeper cannot hold it.
        let tip = self.cfg.keeper_tip.0;
        let mut tip_paid = 0u128;
        if tip > 0 {
            if self.ft.accounts.get(&keeper).is_some() {
                self.ft.internal_deposit(&keeper, tip);
                self.minted += tip;
                tip_paid = tip;
                FtMint {
                    owner_id: &keeper,
                    amount: U128(tip),
                    memo: Some("keeper_tip"),
                }
                .emit();
            } else {
                log!("keeper {} is not registered; tip skipped", keeper);
            }
        }

        let settled_ts = env::block_timestamp() / 1_000_000_000;
        let log_entry = RoundLog {
            round,
            hour: reading.hour,
            mm_tenths: reading.mm_tenths.clone(),
            minted: minted_log,
            burned: burned_log,
            jackpot_paid: jackpot_log,
            sprouted: sprouted_log,
            keeper: keeper.clone(),
            keeper_tip: U128(tip_paid),
            outlayer_request_id: reading.outlayer_request_id,
            tick_block_height: in_flight.tick_block_height,
            settled_ts,
        };
        self.round_logs.insert(&round, &log_entry);

        DrizzleEvents::Round(vec![RoundEvent {
            round,
            hour: reading.hour,
            mm_tenths: reading.mm_tenths,
            minted: log_entry.minted,
            burned: log_entry.burned,
            keeper,
        }])
        .emit();

        self.next_round += 1;
        self.rounds_settled += 1;
        self.in_flight = None;
    }

    // ---------------------------------------------------------------- views

    pub fn get_config(&self) -> Config {
        self.cfg.clone()
    }

    pub fn get_round_state(&self) -> RoundStateView {
        let next_round = self.next_round;
        RoundStateView {
            next_round,
            next_hour: self.cfg.first_hour + next_round,
            due_at: self.due_at(next_round),
            in_flight: self.in_flight.as_ref().map(|f| InFlightView {
                round: f.round,
                started_ts: f.started_ts,
            }),
            now: env::block_timestamp() / 1_000_000_000,
        }
    }

    pub fn get_cities(&self) -> Vec<CityView> {
        self.cfg
            .cities
            .iter()
            .enumerate()
            .map(|(index, city)| {
                let state = &self.cities[index];
                CityView {
                    index: index as u8,
                    name: city.name.clone(),
                    lat_e4: city.lat_e4,
                    lon_e4: city.lon_e4,
                    value: U128(state.value),
                    shares: U128(state.shares),
                    pending_total: U128(state.pending_total),
                    jackpot: U128(state.jackpot),
                    planters: state.planters,
                    last_rain_round: state.last_rain_round,
                    last_mm_tenths: state.last_mm_tenths,
                }
            })
            .collect()
    }

    pub fn get_user(&self, account_id: AccountId) -> UserView {
        let nodes = (0..NUM_CITIES).map(|city| {
            let city = city as u8;
            let position = self
                .users
                .get(&(account_id.clone(), city))
                .unwrap_or_default();
            // Project the pending sprout so the UI never shows stale shares.
            let (shares, pending) = if position.pending > 0
                && position.pending_round < self.next_round
            {
                let ratio = self
                    .sprout_ratio
                    .get(&(position.pending_round, city))
                    .expect("missing sprout ratio");
                (
                    position.shares + pool::sprout_user(position.pending, ratio),
                    0,
                )
            } else {
                (position.shares, position.pending)
            };
            let value = pool::shares_value(&self.cities[city as usize].to_pool(), shares);
            UserCityView {
                index: city,
                shares: U128(shares),
                value: U128(value),
                pending: U128(pending),
                pending_round: if pending > 0 {
                    Some(position.pending_round)
                } else {
                    None
                },
            }
        });
        UserView {
            balance: U128(self.ft.accounts.get(&account_id).unwrap_or(0)),
            seeded: self.seeded.contains(&account_id),
            cities: nodes.collect(),
        }
    }

    pub fn get_round_log(&self, round: u64) -> Option<RoundLog> {
        self.round_logs.get(&round)
    }

    pub fn get_round_logs(&self, from: u64, limit: u32) -> Vec<RoundLog> {
        require!(limit <= MAX_ROUND_LOG_PAGE, "limit must be <= 100");
        (from..from.saturating_add(limit as u64))
            .filter_map(|round| self.round_logs.get(&round))
            .collect()
    }

    pub fn get_stats(&self) -> StatsView {
        StatsView {
            total_supply: U128(self.ft.total_supply),
            minted: U128(self.minted),
            burned: U128(self.burned),
            danced: U128(self.danced),
            seeded_count: self.seeded_count,
            rounds_settled: self.rounds_settled,
        }
    }

    pub fn ft_metadata(&self) -> FungibleTokenMetadata {
        FungibleTokenMetadata {
            spec: "ft-1.0.0".to_string(),
            name: self.cfg.name.clone(),
            symbol: self.cfg.symbol.clone(),
            icon: self.cfg.icon.clone(),
            reference: None,
            reference_hash: None,
            decimals: self.cfg.decimals,
        }
    }

    // ---------------------------------------------------------------- internals

    /// Unix second at which `tick()` for `round` becomes allowed.
    fn due_at(&self, round: u64) -> u64 {
        if self.cfg.replay {
            self.cfg.genesis_ts + (round + 1) * self.cfg.round_seconds
        } else {
            (self.cfg.first_hour + round + 1) * 3600 + LIVE_GRACE_SECS
        }
    }

    fn assert_city(&self, city: u8) {
        require!((city as usize) < NUM_CITIES, "invalid city");
    }

    /// Lazily converts settled pending into shares at the stored round ratio.
    fn sprout_position(&self, position: &mut UserCity, city: u8) {
        if position.pending == 0 || position.pending_round >= self.next_round {
            return;
        }
        let ratio = self
            .sprout_ratio
            .get(&(position.pending_round, city))
            .expect("missing sprout ratio");
        position.shares += pool::sprout_user(position.pending, ratio);
        position.pending = 0;
        position.pending_round = 0;
    }

    fn credit(&mut self, account_id: &AccountId, amount: u128) {
        if amount == 0 {
            return;
        }
        self.ft
            .internal_transfer(&env::current_account_id(), account_id, amount, Some("uproot".to_string()));
    }

    fn fail_round(&mut self, round: u64, reason: &str) {
        log!("round {} failed: {}", round, reason);
        self.in_flight = None;
        DrizzleEvents::RoundFailed(vec![RoundFailedEvent {
            round,
            reason: reason.to_string(),
        }])
        .emit();
    }

    /// Minimum storage bond: one FT account record plus 12 `UserCity` entries.
    fn storage_min(&self) -> NearToken {
        env::storage_byte_cost()
            .saturating_mul((self.ft.account_storage_usage + self.user_extra_storage) as u128)
    }

    /// Registers `account_id` when needed, enforcing (and refunding) the bond.
    fn deposit_registration(&mut self, account_id: &AccountId) {
        let amount = env::attached_deposit();
        let min = self.storage_min();
        if self.ft.accounts.contains_key(account_id) {
            if amount > NearToken::from_yoctonear(0) {
                Promise::new(env::predecessor_account_id())
                    .transfer(amount)
                    .detach();
            }
            return;
        }
        require!(
            amount >= min,
            "attached deposit is less than the minimum storage balance"
        );
        self.ft.internal_register_account(account_id);
        let refund = amount.saturating_sub(min);
        if refund > NearToken::from_yoctonear(0) {
            Promise::new(env::predecessor_account_id())
                .transfer(refund)
                .detach();
        }
    }
}

// ---------------------------------------------------------------- NEP-141

#[near]
impl FungibleTokenCore for Contract {
    #[payable]
    fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>) {
        self.ft.ft_transfer(receiver_id, amount, memo)
    }

    #[payable]
    fn ft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        amount: U128,
        memo: Option<String>,
        msg: String,
    ) -> PromiseOrValue<U128> {
        self.ft.ft_transfer_call(receiver_id, amount, memo, msg)
    }

    fn ft_total_supply(&self) -> U128 {
        self.ft.ft_total_supply()
    }

    fn ft_balance_of(&self, account_id: AccountId) -> U128 {
        self.ft.ft_balance_of(account_id)
    }
}

#[near]
impl FungibleTokenResolver for Contract {
    #[private]
    fn ft_resolve_transfer(
        &mut self,
        sender_id: AccountId,
        receiver_id: AccountId,
        amount: U128,
    ) -> U128 {
        let (used, burned) =
            self.ft
                .internal_ft_resolve_transfer(&sender_id, receiver_id, amount);
        if burned > 0 {
            self.burned += burned;
            FtBurn {
                owner_id: &sender_id,
                amount: U128(burned),
                memo: Some("resolve_transfer"),
            }
            .emit();
        }
        U128(used)
    }
}

// ---------------------------------------------------------------- NEP-145

#[near]
impl StorageManagement for Contract {
    #[payable]
    fn storage_deposit(
        &mut self,
        account_id: Option<AccountId>,
        registration_only: Option<bool>,
    ) -> StorageBalance {
        let amount = env::attached_deposit();
        let account_id = account_id.unwrap_or_else(env::predecessor_account_id);
        let min = self.storage_min();
        if self.ft.accounts.contains_key(&account_id) {
            log!("The account is already registered, refunding the deposit");
            if amount > NearToken::from_yoctonear(0) {
                Promise::new(env::predecessor_account_id())
                    .transfer(amount)
                    .detach();
            }
        } else {
            require!(
                amount >= min,
                "The attached deposit is less than the minimum storage balance"
            );
            self.ft.internal_register_account(&account_id);
            let refund = amount.saturating_sub(min);
            if refund > NearToken::from_yoctonear(0) {
                Promise::new(env::predecessor_account_id())
                    .transfer(refund)
                    .detach();
            }
        }
        let _ = registration_only;
        StorageBalance {
            total: min,
            available: NearToken::from_yoctonear(0),
        }
    }

    #[payable]
    fn storage_withdraw(&mut self, amount: Option<NearToken>) -> StorageBalance {
        assert_one_yocto();
        let account_id = env::predecessor_account_id();
        require!(
            self.ft.accounts.contains_key(&account_id),
            "The account is not registered"
        );
        if let Some(amount) = amount {
            require!(
                amount.as_yoctonear() == 0,
                "The amount is greater than the available storage balance"
            );
        }
        StorageBalance {
            total: self.storage_min(),
            available: NearToken::from_yoctonear(0),
        }
    }

    #[payable]
    fn storage_unregister(&mut self, force: Option<bool>) -> bool {
        assert_one_yocto();
        let account_id = env::predecessor_account_id();
        let Some(balance) = self.ft.accounts.get(&account_id) else {
            log!("The account {} is not registered", account_id);
            return false;
        };
        if balance > 0 && !force.unwrap_or(false) {
            env::panic_str(
                "Can't unregister the account with the positive balance without force",
            );
        }
        for city in 0..NUM_CITIES {
            let position = self.users.get(&(account_id.clone(), city as u8));
            if let Some(position) = position {
                require!(
                    position.is_empty(),
                    "uproot your city positions before unregistering"
                );
            }
        }
        for city in 0..NUM_CITIES {
            self.users.remove(&(account_id.clone(), city as u8));
        }
        self.ft.accounts.remove(&account_id);
        self.ft.total_supply -= balance;
        if balance > 0 {
            self.burned += balance;
            FtBurn {
                owner_id: &account_id,
                amount: U128(balance),
                memo: Some("storage_unregister"),
            }
            .emit();
        }
        // `seeded` is kept on purpose: re-registering must not allow a second seed.
        let refund = self
            .storage_min()
            .saturating_add(NearToken::from_yoctonear(1));
        Promise::new(account_id).transfer(refund).detach();
        true
    }

    fn storage_balance_bounds(&self) -> StorageBalanceBounds {
        let min = self.storage_min();
        StorageBalanceBounds {
            min,
            max: Some(min),
        }
    }

    fn storage_balance_of(&self, account_id: AccountId) -> Option<StorageBalance> {
        self.ft.accounts.contains_key(&account_id).then(|| StorageBalance {
            total: self.storage_min(),
            available: NearToken::from_yoctonear(0),
        })
    }
}

// ---------------------------------------------------------------- unit tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        City, ExecutionSource, OracleConfig, ResourceLimits, WasmUrlSource,
    };
    use near_sdk::serde_json::json;
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::testing_env;
    use near_sdk::{PromiseError, PromiseResult, RuntimeFeesConfig};
    use std::collections::HashMap;

    const DEC: u128 = 1_000_000_000_000_000_000;
    const CONTRACT_ID: &str = "drizzle.testnet";
    const ORACLE_DEPOSIT: u128 = 10_000_000_000_000_000_000_000; // 0.01 NEAR
    const GENESIS: u64 = 1_000;
    const FIRST_HOUR: u64 = 497_280;
    const ROUND_SECS: u64 = 60;

    fn acct(name: &str) -> AccountId {
        name.parse().unwrap()
    }

    fn city_vec() -> Vec<City> {
        [
            ("Bergen", 603_900, 53_200),
            ("Sao Paulo", -235_500, -466_300),
            ("Lagos", 65_200, 33_800),
            ("Singapore", 13_500, 1_038_200),
            ("Tokyo", 356_800, 1_396_900),
            ("Jakarta", -62_000, 1_068_500),
            ("Mumbai", 190_800, 728_800),
            ("London", 515_100, -1_300),
            ("Lisbon", 387_200, -91_400),
            ("Seattle", 476_100, -1_223_300),
            ("Cairo", 300_400, 312_400),
            ("Dubai", 252_000, 552_700),
        ]
        .iter()
        .map(|(name, lat, lon)| City {
            name: (*name).to_string(),
            lat_e4: *lat,
            lon_e4: *lon,
        })
        .collect()
    }

    fn test_config() -> Config {
        Config {
            name: "Drizzle".to_string(),
            symbol: "DRIZZLE".to_string(),
            decimals: 18,
            icon: None,
            cities: city_vec(),
            emission_per_round: U128(100 * DEC),
            evap_bps: 10,
            rain_threshold_tenths: 2,
            cap_tenths: 50,
            seed_amount: U128(100 * DEC),
            keeper_tip: U128(1 * DEC),
            round_seconds: ROUND_SECS,
            genesis_ts: GENESIS,
            first_hour: FIRST_HOUR,
            replay: true,
            oracle: OracleConfig {
                outlayer: acct("outlayer.testnet"),
                source: ExecutionSource::WasmUrl(WasmUrlSource {
                    url: "https://test.fastfs.io/drizzle-dev.testnet/outlayer.testnet/pinned.wasm"
                        .to_string(),
                    hash: "7d708ee751d708e8bfd331785204190e1741ed1ca0ce5f76dcf4317e75a7128b"
                        .to_string(),
                    build_target: "wasm32-wasip2".to_string(),
                }),
                resource_limits: ResourceLimits {
                    max_instructions: 5_000_000_000,
                    max_memory_mb: 64,
                    max_execution_seconds: 20,
                },
                deposit: U128(ORACLE_DEPOSIT),
            },
        }
    }

    fn context(who: &str, ts: u64, deposit: u128) -> near_sdk::VMContext {
        let mut builder = VMContextBuilder::new();
        builder
            .current_account_id(acct(CONTRACT_ID))
            .signer_account_id(acct(who))
            .predecessor_account_id(acct(who))
            .block_timestamp(ts * 1_000_000_000)
            .block_height(7)
            .attached_deposit(NearToken::from_yoctonear(deposit));
        builder.build()
    }

    fn set_env(who: &str, ts: u64, deposit: u128) {
        testing_env!(context(who, ts, deposit));
    }

    /// Switches to a callback context with mocked promise results.
    fn callback_env(ts: u64, result: PromiseResult) {
        callback_env_as(CONTRACT_ID, ts, result);
    }

    /// Callback context with an arbitrary predecessor (to test the private gate).
    fn callback_env_as(who: &str, ts: u64, result: PromiseResult) {
        let ctx = context(who, ts, 0);
        testing_env!(
            ctx,
            near_sdk::test_vm_config(),
            RuntimeFeesConfig::test(),
            HashMap::default(),
            vec![result],
        );
    }

    /// The same deserialization the generated `on_weather` wrapper performs.
    fn callback_result() -> Result<Option<Value>, PromiseError> {
        env::promise_result_checked(0, 8 * 1024 * 1024)
            .map(|data| near_sdk::serde_json::from_slice::<Option<Value>>(&data).unwrap())
    }

    fn reset_storage() {
        near_sdk::mock::with_mocked_blockchain(|b| {
            let _ = b.take_storage();
        });
    }

    fn setup() -> Contract {
        reset_storage();
        set_env("alice.testnet", GENESIS, 0);
        Contract::new(test_config())
    }

    fn seed(c: &mut Contract, who: &str, ts: u64) {
        let deposit = c.storage_balance_bounds().min.as_yoctonear();
        set_env(who, ts, deposit);
        c.claim_seed();
    }

    #[test]
    #[should_panic(expected = "already seeded")]
    fn unregister_then_reregister_cannot_seed_twice() {
        let mut c = setup();
        seed(&mut c, "bob.testnet", GENESIS + 1);
        let supply = c.ft.total_supply;
        set_env("bob.testnet", GENESIS + 2, 1);
        assert!(c.storage_unregister(Some(true)));
        assert_eq!(c.ft.total_supply, supply - 100 * DEC);
        assert_eq!(c.burned, 100 * DEC);
        seed(&mut c, "bob.testnet", GENESIS + 3);
    }

    fn plant(c: &mut Contract, who: &str, city: u8, amount: u128, ts: u64) {
        set_env(who, ts, 1);
        c.plant(city, U128(amount));
    }

    fn uproot(c: &mut Contract, who: &str, city: u8, shares: Option<u128>, ts: u64) -> u128 {
        set_env(who, ts, 1);
        c.uproot(city, shares.map(U128)).0
    }

    fn rain_dance(c: &mut Contract, who: &str, city: u8, amount: u128, ts: u64) {
        set_env(who, ts, 1);
        c.rain_dance(city, U128(amount));
    }

    fn payload(round: u64, mm: &[u16; NUM_CITIES]) -> Vec<u8> {
        near_sdk::serde_json::to_vec(&json!({
            "hour": FIRST_HOUR + round,
            "mm_tenths": mm,
            "source": "open-meteo",
            "outlayer_request_id": format!("{}", 22_000 + round),
        }))
        .unwrap()
    }

    /// Ticks `next_round`, then delivers a successful callback with `mm`.
    fn settle(c: &mut Contract, mm: &[u16; NUM_CITIES], ts: u64, keeper: &str) {
        let round = c.next_round;
        let keeper_id = acct(keeper);
        set_env(keeper, ts, ORACLE_DEPOSIT);
        let _ = c.tick();
        callback_env(ts + 5, PromiseResult::Successful(payload(round, mm)));
        let result = callback_result();
        c.on_weather(round, keeper_id, result);
    }

    /// Ticks `next_round`, then delivers raw callback bytes (may be null/garbage).
    fn settle_raw(c: &mut Contract, bytes: Vec<u8>, ts: u64, keeper: &str) {
        let round = c.next_round;
        let keeper_id = acct(keeper);
        set_env(keeper, ts, ORACLE_DEPOSIT);
        let _ = c.tick();
        callback_env(ts + 5, PromiseResult::Successful(bytes));
        let result = callback_result();
        c.on_weather(round, keeper_id, result);
    }

    fn zero_mm() -> [u16; NUM_CITIES] {
        [0u16; NUM_CITIES]
    }

    fn planters(c: &Contract, city: u8) -> u32 {
        c.cities[city as usize].planters
    }

    /// Invariants 1–2 of HANDOFF §2.2, checked against the contract state.
    fn assert_invariants(c: &Contract, users: &[AccountId]) {
        let contract_id = acct(CONTRACT_ID);
        let contract_balance = c.ft.accounts.get(&contract_id).unwrap_or(0);
        let mut sum = contract_balance;
        for user in users {
            sum += c.ft.accounts.get(user).unwrap_or(0);
        }
        assert_eq!(
            sum, c.ft.total_supply,
            "invariant 1: total_supply != sum of balances"
        );

        let held = c
            .cities
            .iter()
            .map(|city| city.value + city.pending_total)
            .sum::<u128>();
        assert_eq!(
            contract_balance, held,
            "invariant 1: contract balance != sum(pool.value) + sum(pending_total)"
        );

        let mut user_shares = 0u128;
        for user in users {
            for city in 0..NUM_CITIES {
                let position = c
                    .users
                    .get(&(user.clone(), city as u8))
                    .unwrap_or_default();
                let shares = if position.pending > 0 && position.pending_round < c.next_round {
                    let ratio = c
                        .sprout_ratio
                        .get(&(position.pending_round, city as u8))
                        .expect("pending past its round must have a sprout ratio");
                    position.shares + pool::sprout_user(position.pending, ratio)
                } else {
                    position.shares
                };
                user_shares += shares;
            }
        }
        let pool_shares: u128 = c.cities.iter().map(|city| city.shares).sum();
        assert_eq!(user_shares, pool_shares, "shares in pools != shares in positions");
    }

    fn users() -> Vec<AccountId> {
        vec![
            acct("alice.testnet"),
            acct("bob.testnet"),
            acct("carol.testnet"),
        ]
    }

    // ------------------------------------------------------------ invariants

    #[test]
    fn full_lifecycle_keeps_supply_balanced() {
        let mut c = setup();
        let u = users();
        assert_invariants(&c, &u);

        for (i, who) in ["alice.testnet", "bob.testnet", "carol.testnet"]
            .iter()
            .enumerate()
        {
            seed(&mut c, who, GENESIS + i as u64);
        }
        assert_invariants(&c, &u);
        assert_eq!(c.get_stats().seeded_count, 3);

        plant(&mut c, "alice.testnet", 0, 100 * DEC, GENESIS + 10);
        plant(&mut c, "bob.testnet", 3, 40 * DEC, GENESIS + 10);
        assert_invariants(&c, &u);

        let mut mm = zero_mm();
        mm[0] = 10;
        settle(&mut c, &mm, GENESIS + ROUND_SECS, "alice.testnet");
        assert_invariants(&c, &u);

        mm[0] = 20;
        mm[3] = 5;
        settle(&mut c, &mm, GENESIS + 2 * ROUND_SECS, "bob.testnet");
        assert_invariants(&c, &u);

        let got = uproot(&mut c, "alice.testnet", 0, None, GENESIS + 130);
        assert!(got > 100 * DEC, "alice should have earned something");
        assert_invariants(&c, &u);

        rain_dance(&mut c, "bob.testnet", 5, 10 * DEC, GENESIS + 131);
        assert_invariants(&c, &u);

        uproot(&mut c, "bob.testnet", 3, None, GENESIS + 132);
        assert_invariants(&c, &u);
    }

    #[test]
    fn planting_in_round_r_earns_nothing_from_round_r() {
        let mut c = setup();
        let u = users();
        seed(&mut c, "alice.testnet", GENESIS);
        plant(&mut c, "alice.testnet", 0, 100 * DEC, GENESIS + 10);

        let mut mm = zero_mm();
        mm[0] = 30;
        settle(&mut c, &mm, GENESIS + ROUND_SECS, "alice.testnet");
        assert_invariants(&c, &u);

        let view = c.get_user(acct("alice.testnet"));
        let node = &view.cities[0];
        assert_eq!(node.pending.0, 0, "pending sprouted");
        assert_eq!(node.shares.0, 100 * DEC, "sprout is 1:1 for an empty pool");
        assert_eq!(
            node.value.0,
            100 * DEC,
            "planting in round r must earn nothing from round r"
        );

        // The next rainy round does pay.
        settle(&mut c, &mm, GENESIS + 2 * ROUND_SECS, "alice.testnet");
        let node = &c.get_user(acct("alice.testnet")).cities[0];
        assert_eq!(node.value.0, 200 * DEC, "alice earns the round-1 emission");
        assert_invariants(&c, &u);
    }

    #[test]
    fn zero_share_rain_mints_nothing() {
        let mut c = setup();
        let u = users();
        seed(&mut c, "alice.testnet", GENESIS);
        rain_dance(&mut c, "alice.testnet", 0, 5 * DEC, GENESIS + 1);
        assert_eq!(c.cities[0].jackpot, 5 * DEC);

        let supply_before = c.ft.total_supply;
        let mut mm = zero_mm();
        mm[0] = 50;
        settle(&mut c, &mm, GENESIS + ROUND_SECS, "alice.testnet");
        assert_invariants(&c, &u);

        let log = c.get_round_log(0).unwrap();
        assert_eq!(log.minted[0].0, 0, "rain with zero shares mints nothing");
        assert_eq!(c.cities[0].jackpot, 5 * DEC, "jackpot stays");
        assert_eq!(c.cities[0].value, 0);
        assert_eq!(
            c.ft.total_supply - supply_before,
            DEC,
            "only the keeper tip is minted"
        );
    }

    #[test]
    fn dry_round_burns_exactly_evap_bps() {
        let mut c = setup();
        let u = users();
        seed(&mut c, "alice.testnet", GENESIS);
        plant(&mut c, "alice.testnet", 0, 100 * DEC, GENESIS + 1);
        let mut mm = zero_mm();
        mm[0] = 10;
        settle(&mut c, &mm, GENESIS + ROUND_SECS, "alice.testnet");
        let value_before = c.cities[0].value;
        assert!(value_before > 0);

        settle(&mut c, &zero_mm(), GENESIS + 2 * ROUND_SECS, "alice.testnet");
        let log = c.get_round_log(1).unwrap();
        assert_eq!(log.burned[0].0, value_before * 10 / 10_000);
        assert_eq!(c.cities[0].value, value_before - value_before * 10 / 10_000);
        assert_invariants(&c, &u);
    }

    #[test]
    fn only_the_callback_can_apply_weather() {
        let mut c = setup();
        let round = c.next_round;
        callback_env_as(
            "alice.testnet",
            GENESIS + ROUND_SECS,
            PromiseResult::Successful(payload(round, &zero_mm())),
        );
        let result = callback_result();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.on_weather(round, acct("alice.testnet"), result);
        }));
        assert!(outcome.is_err(), "a non-self predecessor must panic");
        assert_eq!(c.next_round, 0);
    }

    #[test]
    fn stale_and_out_of_order_callbacks_are_ignored() {
        let mut c = setup();
        let u = users();
        seed(&mut c, "alice.testnet", GENESIS);
        let mut mm = zero_mm();
        mm[0] = 10;

        // Settle round 0 properly.
        settle(&mut c, &mm, GENESIS + ROUND_SECS, "alice.testnet");
        assert_eq!(c.next_round, 1);
        let supply = c.ft.total_supply;
        let log = c.get_round_log(0).unwrap();

        // A duplicate callback for round 0 (in flight is empty now) is ignored.
        callback_env(GENESIS + 130, PromiseResult::Successful(payload(0, &mm)));
        let result = callback_result();
        c.on_weather(0, acct("alice.testnet"), result);
        assert_eq!(c.next_round, 1);
        assert_eq!(c.ft.total_supply, supply);
        assert!(c.in_flight.is_none());

        // Start round 1, then deliver a callback for the *wrong* round.
        set_env("alice.testnet", GENESIS + 2 * ROUND_SECS, ORACLE_DEPOSIT);
        let _ = c.tick();
        let round_1_supply = c.ft.total_supply;
        callback_env(GENESIS + 130, PromiseResult::Successful(payload(0, &mm)));
        let result = callback_result();
        c.on_weather(0, acct("alice.testnet"), result);
        assert_eq!(c.next_round, 1, "stale callback must not settle");
        assert_eq!(c.ft.total_supply, round_1_supply);
        assert_eq!(c.in_flight.as_ref().unwrap().round, 1);
        assert!(c.get_round_log(1).is_none());

        // And a callback for a future round is ignored as well.
        callback_env(
            GENESIS + 2 * ROUND_SECS + 1,
            PromiseResult::Successful(payload(5, &mm)),
        );
        let result = callback_result();
        c.on_weather(5, acct("alice.testnet"), result);
        assert_eq!(c.next_round, 1);
        assert!(c.get_round_log(1).is_none());
        assert_invariants(&c, &u);
        let _ = log;
    }

    #[test]
    fn tick_requires_due_and_is_single_flight_for_600s() {
        let mut c = setup();

        // Before due.
        set_env("alice.testnet", GENESIS + ROUND_SECS - 1, ORACLE_DEPOSIT);
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = c.tick();
        }))
        .is_err());

        // At due time it works.
        set_env("alice.testnet", GENESIS + ROUND_SECS, ORACLE_DEPOSIT);
        let _ = c.tick();
        assert_eq!(c.in_flight.as_ref().unwrap().round, 0);

        // A second tick while in flight is rejected.
        set_env("bob.testnet", GENESIS + ROUND_SECS + 1, ORACLE_DEPOSIT);
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = c.tick();
        }))
        .is_err());

        // After 600 s the stuck request expires and a retry is allowed.
        set_env("bob.testnet", GENESIS + ROUND_SECS + 600, ORACLE_DEPOSIT);
        let _ = c.tick();
        assert_eq!(c.in_flight.as_ref().unwrap().round, 0);
        assert_eq!(c.in_flight.as_ref().unwrap().started_ts, GENESIS + ROUND_SECS + 600);
        assert_eq!(c.next_round, 0);

        // The retried callback settles normally.
        callback_env(
            GENESIS + ROUND_SECS + 601,
            PromiseResult::Successful(payload(0, &zero_mm())),
        );
        let result = callback_result();
        c.on_weather(0, acct("bob.testnet"), result);
        assert_eq!(c.next_round, 1);
        assert!(c.in_flight.is_none());
    }

    #[test]
    fn failed_callbacks_do_not_advance_the_round() {
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("null", b"null".to_vec()),
            ("garbage", b"\"nope\"".to_vec()),
            (
                "wrong hour",
                near_sdk::serde_json::to_vec(&json!({
                    "hour": FIRST_HOUR + 9,
                    "mm_tenths": zero_mm().to_vec(),
                    "source": "open-meteo"
                }))
                .unwrap(),
            ),
            (
                "short mm",
                near_sdk::serde_json::to_vec(&json!({
                    "hour": FIRST_HOUR,
                    "mm_tenths": vec![0u16; 3],
                    "source": "open-meteo"
                }))
                .unwrap(),
            ),
        ];

        for (name, bytes) in cases {
            let mut c = setup();
            let u = users();
            seed(&mut c, "alice.testnet", GENESIS);
            plant(&mut c, "alice.testnet", 0, 10 * DEC, GENESIS + 1);
            let supply = c.ft.total_supply;
            settle_raw(&mut c, bytes, GENESIS + ROUND_SECS, "alice.testnet");
            assert_eq!(c.next_round, 0, "{name} must not advance the round");
            assert!(c.in_flight.is_none(), "{name} must clear in_flight");
            assert_eq!(c.ft.total_supply, supply, "{name} must not change supply");
            assert!(c.get_round_log(0).is_none());
            assert_invariants(&c, &u);

            // The keeper can immediately retry the same round.
            let retry_round = c.next_round;
            set_env("alice.testnet", GENESIS + ROUND_SECS + 1, ORACLE_DEPOSIT);
            let _ = c.tick();
            assert_eq!(c.in_flight.as_ref().unwrap().round, retry_round);
        }
    }

    #[test]
    fn uproot_returns_pending_and_sweeps_the_last_holder() {
        let mut c = setup();
        let u = users();
        seed(&mut c, "alice.testnet", GENESIS);
        plant(&mut c, "alice.testnet", 0, 90 * DEC, GENESIS + 1);
        let mut mm = zero_mm();
        mm[0] = 10;
        settle(&mut c, &mm, GENESIS + ROUND_SECS, "alice.testnet");
        assert_eq!(planters(&c, 0), 1);

        plant(&mut c, "alice.testnet", 0, 10 * DEC, GENESIS + 70);
        assert_eq!(planters(&c, 0), 1);

        let balance_before = c.ft.accounts.get(&acct("alice.testnet")).unwrap();
        let value = c.cities[0].value + c.cities[0].pending_total;
        let got = uproot(&mut c, "alice.testnet", 0, None, GENESIS + 71);
        assert_eq!(got, value, "last holder sweeps the whole pool value");
        assert_eq!(
            c.ft.accounts.get(&acct("alice.testnet")).unwrap(),
            balance_before + got
        );
        assert_eq!(c.cities[0].value, 0);
        assert_eq!(c.cities[0].shares, 0);
        assert_eq!(c.cities[0].pending_total, 0);
        assert_eq!(planters(&c, 0), 0);
        assert_invariants(&c, &u);
    }

    #[test]
    fn partial_uproot_keeps_the_pool_alive() {
        let mut c = setup();
        let u = users();
        seed(&mut c, "alice.testnet", GENESIS);
        seed(&mut c, "bob.testnet", GENESIS);
        plant(&mut c, "alice.testnet", 0, 100 * DEC, GENESIS + 1);
        plant(&mut c, "bob.testnet", 0, 100 * DEC, GENESIS + 1);
        let mut mm = zero_mm();
        mm[0] = 10;
        settle(&mut c, &mm, GENESIS + ROUND_SECS, "alice.testnet");
        assert_eq!(planters(&c, 0), 2);

        let alice_shares = c.get_user(acct("alice.testnet")).cities[0].shares.0;
        let half = alice_shares / 2;
        let got = uproot(&mut c, "alice.testnet", 0, Some(half), GENESIS + 70);
        assert!(got > 0);
        assert_eq!(planters(&c, 0), 2, "still planted");
        assert_eq!(
            c.get_user(acct("alice.testnet")).cities[0].shares.0,
            alice_shares - half
        );
        assert_invariants(&c, &u);
    }

    #[test]
    fn rain_dance_burns_supply_into_the_jackpot() {
        let mut c = setup();
        let u = users();
        seed(&mut c, "alice.testnet", GENESIS);
        let supply = c.ft.total_supply;
        rain_dance(&mut c, "alice.testnet", 2, 7 * DEC, GENESIS + 1);
        assert_eq!(c.ft.total_supply, supply - 7 * DEC);
        assert_eq!(c.cities[2].jackpot, 7 * DEC);
        let stats = c.get_stats();
        assert_eq!(stats.burned.0, 7 * DEC);
        assert_eq!(stats.danced.0, 7 * DEC);
        assert_invariants(&c, &u);
    }

    #[test]
    fn get_user_projects_settled_pending() {
        let mut c = setup();
        let u = users();
        seed(&mut c, "alice.testnet", GENESIS);
        plant(&mut c, "alice.testnet", 0, 30 * DEC, GENESIS + 1);
        // Not settled yet: the view shows pending only.
        let node = &c.get_user(acct("alice.testnet")).cities[0];
        assert_eq!(node.pending.0, 30 * DEC);
        assert_eq!(node.shares.0, 0);
        assert_eq!(node.pending_round, Some(0));

        let mut mm = zero_mm();
        mm[0] = 10;
        settle(&mut c, &mm, GENESIS + ROUND_SECS, "alice.testnet");

        // Sprouted on read without a write.
        let node = &c.get_user(acct("alice.testnet")).cities[0];
        assert_eq!(node.pending.0, 0);
        assert_eq!(node.shares.0, 30 * DEC);
        assert_eq!(node.pending_round, None);
        assert_eq!(node.value.0, 30 * DEC);
        assert_invariants(&c, &u);
    }

    #[test]
    fn seed_is_once_per_account_and_registers_storage() {
        let mut c = setup();
        let min = c.storage_balance_bounds().min;
        // 12 UserCity entries + one FT account row; the mock VM tracks storage.
        assert!(
            (1000..5000).contains(&(min.as_yoctonear() / 10u128.pow(19))),
            "unexpected storage bond: {} yocto",
            min.as_yoctonear()
        );
        // Less than the bond: rejected before any state change.
        set_env("alice.testnet", GENESIS, min.as_yoctonear() / 2);
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.claim_seed();
        }))
        .is_err());
        assert!(c.storage_balance_of(acct("alice.testnet")).is_none());

        set_env("alice.testnet", GENESIS, min.as_yoctonear());
        c.claim_seed();
        assert!(c.storage_balance_of(acct("alice.testnet")).is_some());
        assert_eq!(c.get_user(acct("alice.testnet")).balance.0, 100 * DEC);

        set_env("alice.testnet", GENESIS + 1, min.as_yoctonear());
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.claim_seed();
        }))
        .is_err());
    }

    #[test]
    fn round_logs_paginate_in_order() {
        let mut c = setup();
        for i in 0..3 {
            settle(&mut c, &zero_mm(), GENESIS + ROUND_SECS * (i + 1), "alice.testnet");
        }
        let logs = c.get_round_logs(0, 10);
        assert_eq!(logs.len(), 3);
        assert_eq!(logs.iter().map(|l| l.round).collect::<Vec<_>>(), vec![0, 1, 2]);
        assert_eq!(c.get_round_logs(1, 1).len(), 1);
        assert!(c.get_round_log(9).is_none());
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.get_round_logs(0, 101);
        }))
        .is_err());
    }

    #[test]
    fn live_rounds_are_due_after_the_hour_plus_grace() {
        reset_storage();
        set_env("alice.testnet", 10_000_000, 0);
        let mut cfg = test_config();
        cfg.replay = false;
        cfg.round_seconds = 3600;
        cfg.first_hour = 498_000;
        let c = Contract::new(cfg);
        let state = c.get_round_state();
        // Live: (first_hour + round + 1) * 3600 + 120, independent of genesis_ts.
        assert_eq!(state.due_at, (498_000 + 1) * 3600 + 120);
        assert_eq!(state.next_hour, 498_000);
        assert_eq!(state.next_round, 0);
        assert!(state.now < state.due_at, "test clock must be before the due time");
    }

    #[test]
    fn get_config_roundtrip_shape() {
        let c = setup();
        let value = near_sdk::serde_json::to_value(c.get_config()).unwrap();
        assert_eq!(value["cities"].as_array().unwrap().len(), NUM_CITIES);
        assert_eq!(value["emission_per_round"], "100000000000000000000");
        assert_eq!(value["oracle"]["deposit"], "10000000000000000000000");
        assert_eq!(value["oracle"]["outlayer"], "outlayer.testnet");
        assert!(value["oracle"]["source"]["WasmUrl"]["hash"].is_string());
        assert_eq!(value["oracle"]["resource_limits"]["max_memory_mb"], 64);
        assert_eq!(value["replay"], true);
        let state = near_sdk::serde_json::to_value(c.get_round_state()).unwrap();
        assert_eq!(state["next_round"], 0);
        assert_eq!(state["due_at"], GENESIS + ROUND_SECS);
    }
}
