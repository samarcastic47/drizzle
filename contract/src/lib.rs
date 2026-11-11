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
use crate::user::{Bucket, CityState, UserCity};
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
/// How many settled rounds keep a `RoundLog`. Each log costs ~0.009 NEAR of
/// storage staking, so an unbounded map would eventually brick the contract.
pub const ROUND_LOG_RETENTION: u64 = 240;
/// Live rounds are 3600 s; the data-availability grace after the hour closes.
pub const LIVE_GRACE_SECS: u64 = 120;
/// Smallest plant (1 DRIZZLE). Tiny plants would mint zero shares at a high
/// share price and hand their value to the existing holders.
pub const MIN_PLANT: u128 = 1_000_000_000_000_000_000;
/// A plant must produce at least this many shares at the worst post-settlement
/// price, so its owner cannot be rounded away. Dead shares keep the price sane;
/// this only rejects a plant that would round to zero.
pub const MIN_SHARES_OUT: u128 = 1;

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
    /// `(round, city) -> Bucket`: refcounted pending holders plus the settled
    /// `(pending_in, shares_out)` ratio. Entries are deleted with their last holder,
    /// so the map is bounded by live planting activity, not by the round count.
    pub buckets: LookupMap<(u64, u8), Bucket>,

    pub next_round: u64,
    pub in_flight: Option<InFlight>,

    pub minted: u128,
    pub burned: u128,
    pub danced: u128,
    pub seeded_count: u64,
    pub rounds_settled: u64,

    /// Bytes occupied by one account's 12 `UserCity` entries plus the 12 settled
    /// bucket entries it can pin (measured in `new`).
    pub user_extra_storage: u64,
    /// Bytes of one `seeded` entry (kept forever, so it is part of the bond).
    pub seeded_storage: u64,
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
        let mut buckets: LookupMap<(u64, u8), Bucket> = LookupMap::new(b"bk".to_vec());
        let mut seeded: LookupSet<AccountId> = LookupSet::new(b"s".to_vec());
        // Measure the storage one account's 12 lazily-created position entries and
        // the 12 buckets it can pin occupy, so `storage_balance_bounds().min`
        // covers the worst case without on_weather ever paying for new storage.
        let probe: AccountId = "a".repeat(64).parse().unwrap();
        let before = env::storage_usage();
        for city in 0..NUM_CITIES {
            users.insert(&(probe.clone(), city as u8), &UserCity::default());
            // Worst case: a settled bucket (ratio filled, epoch bumped) with a holder.
            buckets.insert(
                &(0u64, city as u8),
                &Bucket {
                    holders: 1,
                    ratio: Some((u128::MAX, u128::MAX)),
                    epoch: u32::MAX,
                },
            );
        }
        let user_extra_storage = env::storage_usage() - before;
        // `seeded` is kept forever, so the bond must also cover its entry.
        let before_seeded = env::storage_usage();
        seeded.insert(&probe);
        let seeded_storage = env::storage_usage() - before_seeded;
        for city in 0..NUM_CITIES {
            users.remove(&(probe.clone(), city as u8));
            buckets.remove(&(0u64, city as u8));
        }
        seeded.remove(&probe);

        Self {
            ft,
            cities: vec![CityState::default(); NUM_CITIES],
            users,
            seeded,
            round_logs: LookupMap::new(b"rl".to_vec()),
            buckets,
            next_round: 0,
            in_flight: None,
            minted: 0,
            burned: 0,
            danced: 0,
            seeded_count: 0,
            rounds_settled: 0,
            user_extra_storage,
            seeded_storage,
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
        self.assert_round_open();
        let amount = amount.0;
        require!(
            amount >= MIN_PLANT,
            "plant amount below the 1 DRIZZLE minimum"
        );

        let account_id = env::predecessor_account_id();
        let mut position = self.users.get(&(account_id.clone(), city)).unwrap_or_default();
        // Stale shares from a reset pool are worthless and must not count as
        // "already planted" for the planter bookkeeping.
        self.sync_epoch(&mut position, city);
        // Capture this *before* the lazy sprout: a pending that sprouts to zero
        // shares still means the account was already counted as a planter.
        let was_planted = self.position_is_planted(&position, city);
        self.sprout_position(&mut position, city);

        // Worst-case post-settlement price: this round the pool can receive a
        // full emission slice plus its jackpot, which is the fewest shares this
        // amount could ever buy.
        let pool = self.cities[city as usize].to_pool();
        if pool.shares > 0 {
            let worst_value = pool
                .value
                .saturating_add(self.cfg.emission_per_round.0)
                .saturating_add(pool.jackpot);
            if pool::mul_div(amount, pool.shares, worst_value) < MIN_SHARES_OUT {
                let min_amount = pool::mul_div(MIN_SHARES_OUT, worst_value, pool.shares) + 1;
                env::panic_str(&format!(
                    "city share price too high for this amount; plant at least {min_amount}"
                ));
            }
        }

        self.ft.internal_transfer(
            &account_id,
            &env::current_account_id(),
            amount,
            Some("plant".to_string()),
        );

        let new_pending = position.pending == 0;
        let mut pool = self.cities[city as usize].to_pool();
        pool::plant(&mut pool, amount);
        let state = &mut self.cities[city as usize];
        state.absorb(&pool);
        if !was_planted {
            state.planters += 1;
        }

        if new_pending {
            position.pending_round = self.next_round;
            self.bucket_add_holder(self.next_round, city);
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
        self.assert_uproot_open(shares);
        let account_id = env::predecessor_account_id();
        let key = (account_id.clone(), city);
        let mut position = self.users.get(&key).unwrap_or_default();
        self.sync_epoch(&mut position, city);
        // Same as `plant`: a pending that sprouted to zero shares still means the
        // account is counted in `planters` and must be decremented here.
        let was_planted = self.position_is_planted(&position, city);
        self.sprout_position(&mut position, city);

        let requested = match shares {
            Some(shares) => u128::from(shares),
            None => position.shares,
        };
        require!(requested <= position.shares, "not enough shares");

        let mut pool = self.cities[city as usize].to_pool();
        let mut credit = pool::uproot(&mut pool, requested);
        position.shares -= requested;

        if shares.is_some() && position.shares > 0 {
            // A dust remainder would be rounded to zero value at the next touch.
            let remaining = pool::shares_value(&pool, position.shares);
            require!(
                remaining >= MIN_PLANT,
                "remaining stake below minimum; uproot all"
            );
        }

        if shares.is_none() && position.pending > 0 {
            let returned_pending = position.pending;
            pool::unplant_pending(&mut pool, returned_pending);
            // The pending never sprouted, so release its (unsettled) bucket holder.
            if position.pending_round >= self.next_round {
                self.bucket_remove_holder(position.pending_round, city);
            }
            position.pending = 0;
            position.pending_round = 0;
            credit += returned_pending;
        }

        let now_planted = self.position_is_planted(&position, city);
        let state = &mut self.cities[city as usize];
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

        let input = oracle::build_input(self.cfg.hour_for_round(round), &self.cfg.cities);
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

        let expected_hour = self.cfg.hour_for_round(round);
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
            // The bucket was created (and refcounted) by `plant`; settlement only
            // fills in its ratio, so on_weather never creates storage entries here.
            if let Some(ratio) = outcome.sprout {
                let key = (round, city as u8);
                let mut bucket = self
                    .buckets
                    .get(&key)
                    .expect("settled round must have a sprout bucket");
                bucket.ratio = Some(ratio);
                if outcome.reset {
                    let epoch = self.cities[city].epoch + 1;
                    self.cities[city].epoch = epoch;
                    bucket.epoch = epoch;
                    // Holders of the dead old shares are no longer planters: only
                    // the pending holders of this round start the new epoch.
                    self.cities[city].planters = bucket.holders;
                } else {
                    bucket.epoch = self.cities[city].epoch;
                }
                self.buckets.insert(&key, &bucket);
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
            hour: self.cfg.hour_for_round(round),
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
        // Keep only the newest `ROUND_LOG_RETENTION` logs.
        if round >= ROUND_LOG_RETENTION {
            self.round_logs.remove(&(round - ROUND_LOG_RETENTION));
        }

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
            next_hour: self.cfg.hour_for_round(next_round),
            oldest_round_log: next_round.saturating_sub(ROUND_LOG_RETENTION),
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
            // Shares from a previous city epoch are worthless.
            let mut shares = if position.shares > 0
                && position.epoch != self.cities[city as usize].epoch
            {
                0
            } else {
                position.shares
            };
            // Project the pending sprout so the UI never shows stale shares.
            let pending = if position.pending > 0 && position.pending_round < self.next_round {
                let bucket = self
                    .buckets
                    .get(&(position.pending_round, city))
                    .expect("missing sprout bucket");
                let ratio = bucket.ratio.expect("sprout bucket was never settled");
                if bucket.epoch == self.cities[city as usize].epoch {
                    shares += pool::sprout_user(position.pending, ratio);
                }
                0
            } else {
                position.pending
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

    /// Live games freeze positions once the round's hour has closed: the weather is
    /// public at that point, so burning (or entering) before settlement would let
    /// active holders dodge evaporation. Replay is exempt (its look-ahead is
    /// inherent and documented); `rain_dance` is never frozen (it cannot profit).
    fn assert_round_open(&self) {
        if self.cfg.replay {
            return;
        }
        let close = (self.cfg.hour_for_round(self.next_round) + 1) * 3600;
        require!(
            env::block_timestamp() / 1_000_000_000 < close,
            "round closing: wait for settlement"
        );
    }

    /// `uproot(None)` may still exit in live mode if settlement is more than a day
    /// late; entering and partial uproots stay frozen.
    fn assert_uproot_open(&self, shares: Option<U128>) {
        if self.cfg.replay {
            return;
        }
        let close = (self.cfg.hour_for_round(self.next_round) + 1) * 3600;
        let now = env::block_timestamp() / 1_000_000_000;
        if now < close {
            return;
        }
        require!(
            shares.is_none() && now >= close.saturating_add(86_400),
            "round closing: wait for settlement"
        );
    }

    /// Lazily converts settled pending into shares at the stored round ratio.
    /// Clears shares that belong to a previous city epoch (reset pools).
    fn sync_epoch(&self, position: &mut UserCity, city: u8) {
        let epoch = self.cities[city as usize].epoch;
        if position.shares > 0 && position.epoch != epoch {
            position.shares = 0;
        }
        position.epoch = epoch;
    }

    /// Whether the account currently counts as a planter of this city.
    ///
    /// Pending that settled before a reset belongs to the dead epoch: it will sprout
    /// to zero shares and the account is not in `planters` (the reset assigned the
    /// counter to that round's bucket holders only).
    fn position_is_planted(&self, position: &UserCity, city: u8) -> bool {
        if position.shares > 0 {
            return true;
        }
        if position.pending == 0 {
            return false;
        }
        if position.pending_round >= self.next_round {
            return true;
        }
        self.buckets
            .get(&(position.pending_round, city))
            .map(|bucket| bucket.epoch == self.cities[city as usize].epoch)
            .unwrap_or(false)
    }

    /// Lazily converts settled pending into shares using its bucket's ratio.
    ///
    /// The bucket is refcounted by its pending holders, so it disappears with the
    /// last one; this keeps the bucket map bounded by live positions.
    fn sprout_position(&mut self, position: &mut UserCity, city: u8) {
        if position.pending == 0 || position.pending_round >= self.next_round {
            return;
        }
        let round = position.pending_round;
        let bucket = self
            .buckets
            .get(&(round, city))
            .expect("missing sprout bucket");
        let ratio = bucket.ratio.expect("sprout bucket was never settled");
        // Pending that settled before a reset belongs to the dead epoch: the pool
        // burned its value, so it sprouts to zero shares.
        if bucket.epoch == self.cities[city as usize].epoch {
            position.shares += pool::sprout_user(position.pending, ratio);
            position.epoch = bucket.epoch;
        }
        position.pending = 0;
        position.pending_round = 0;
        self.bucket_remove_holder(round, city);
    }

    fn bucket_add_holder(&mut self, round: u64, city: u8) {
        let key = (round, city);
        let mut bucket = self.buckets.get(&key).unwrap_or_default();
        bucket.holders = bucket
            .holders
            .checked_add(1)
            .unwrap_or_else(|| env::abort());
        self.buckets.insert(&key, &bucket);
    }

    fn bucket_remove_holder(&mut self, round: u64, city: u8) {
        let key = (round, city);
        let mut bucket = self
            .buckets
            .get(&key)
            .expect("bucket should exist while it has holders");
        bucket.holders = bucket.holders.saturating_sub(1);
        if bucket.holders == 0 {
            self.buckets.remove(&key);
        } else {
            self.buckets.insert(&key, &bucket);
        }
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

    /// What an account must deposit to register: the full bond, minus the
    /// `seeded` entry's stake when that entry already exists (it was paid once).
    fn registration_min(&self, account_id: &AccountId) -> NearToken {
        let mut min = self.storage_min();
        if self.seeded.contains(account_id) {
            min = min.saturating_sub(
                env::storage_byte_cost().saturating_mul(self.seeded_storage as u128),
            );
        }
        min
    }

    /// Minimum storage bond: one FT account record plus 12 `UserCity` and 12
    /// bucket entries.
    fn storage_min(&self) -> NearToken {
        env::storage_byte_cost()
            .saturating_mul(
                (self.ft.account_storage_usage + self.user_extra_storage + self.seeded_storage)
                    as u128,
            )
    }

    /// Registers `account_id` when needed, enforcing (and refunding) the bond.
    fn deposit_registration(&mut self, account_id: &AccountId) {
        let amount = env::attached_deposit();
        let min = self.registration_min(account_id);
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
        // Otherwise the tokens would land in the contract's own balance, which is
        // reserved for pool value + pending and has no rescue path.
        require!(
            receiver_id != env::current_account_id(),
            "can't transfer to the Drizzle contract; use plant"
        );
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
        require!(
            receiver_id != env::current_account_id(),
            "can't transfer to the Drizzle contract; use plant"
        );
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
        // The standard library already emits `ft_burn` for this supply reduction;
        // only the accounting counter lives here.
        if burned > 0 {
            self.burned += burned;
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
        let min = self.registration_min(&account_id);
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
        // The account's balance view and the bounds stay at the full bond; only
        // the charge skipped the retained `seeded` entry.
        StorageBalance {
            total: self.storage_min(),
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
        // `seeded` is kept on purpose: re-registering must not allow a second seed,
        // so its storage stake is not refunded.
        let mut refund = self.storage_min();
        if self.seeded.contains(&account_id) {
            refund = refund.saturating_sub(
                env::storage_byte_cost().saturating_mul(self.seeded_storage as u128),
            );
        }
        let refund = refund.saturating_add(NearToken::from_yoctonear(1));
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
    const REPLAY_HOURS: u64 = 168;

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
            replay_hours: REPLAY_HOURS,
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
            "hour": FIRST_HOUR + round % REPLAY_HOURS,
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

        for city in 0..NUM_CITIES {
            let state = &c.cities[city];
            let mut city_user_shares = 0u128;
            for user in users {
                let position = c
                    .users
                    .get(&(user.clone(), city as u8))
                    .unwrap_or_default();
                let mut shares = if position.shares > 0 && position.epoch != state.epoch {
                    0
                } else {
                    position.shares
                };
                if position.pending > 0 && position.pending_round < c.next_round {
                    let bucket = c
                        .buckets
                        .get(&(position.pending_round, city as u8))
                        .expect("pending past its round must have a bucket");
                    let ratio = bucket.ratio.expect("settled bucket has a ratio");
                    if bucket.epoch == state.epoch {
                        shares += pool::sprout_user(position.pending, ratio);
                    }
                }
                city_user_shares += shares;
            }
            assert!(
                city_user_shares <= state.shares,
                "city {city}: user shares {city_user_shares} > pool shares {}",
                state.shares
            );
            let unowned = state.shares - city_user_shares;
            assert!(
                unowned == 0 || unowned == pool::DEAD_SHARES,
                "city {city}: {unowned} unowned (non-dead) shares"
            );
        }
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
        assert!(
            node.value.0 <= 100 * DEC && node.value.0 + 2 * pool::DEAD_SHARES >= 100 * DEC,
            "planting in round r must earn nothing from round r (got {})",
            node.value.0
        );

        // The next rainy round does pay.
        settle(&mut c, &mm, GENESIS + 2 * ROUND_SECS, "alice.testnet");
        let node = &c.get_user(acct("alice.testnet")).cities[0];
        assert!(
            node.value.0 <= 200 * DEC && node.value.0 + 2 * pool::DEAD_SHARES >= 200 * DEC,
            "alice earns the round-1 emission (got {})",
            node.value.0
        );
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
        assert!(
            node.value.0 <= 30 * DEC && node.value.0 + 2 * pool::DEAD_SHARES >= 30 * DEC,
            "sprouted value should be ~30 DRIZZLE (got {})",
            node.value.0
        );
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
        // The bond covers the FT row, 12 positions, 12 settled buckets and `seeded`.
        let covered = (c.ft.account_storage_usage
            + c.user_extra_storage
            + c.seeded_storage) as u128;
        assert!(c.seeded_storage > 0);
        assert_eq!(
            min.as_yoctonear(),
            env::storage_byte_cost().as_yoctonear() * covered
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
        let now = 1_795_000_000u64;
        set_env("alice.testnet", now, 0);
        let hour = now / 3600;
        let mut cfg = test_config();
        cfg.replay = false;
        cfg.replay_hours = 0;
        cfg.round_seconds = 3600;
        cfg.first_hour = hour;
        let c = Contract::new(cfg);
        let state = c.get_round_state();
        // Live: (first_hour + round + 1) * 3600 + 120, independent of genesis_ts.
        assert_eq!(state.due_at, (hour + 1) * 3600 + 120);
        assert_eq!(state.next_hour, hour);
        assert_eq!(state.next_round, 0);
        assert!(state.now < state.due_at, "test clock must be before the due time");
    }

    #[test]
    fn replay_hours_loop_the_historical_week() {
        let c = setup();
        assert_eq!(c.cfg.hour_for_round(167), FIRST_HOUR + 167);
        assert_eq!(c.cfg.hour_for_round(168), FIRST_HOUR);
        assert_eq!(c.cfg.hour_for_round(400), FIRST_HOUR + 64);
        assert_eq!(c.get_round_state().next_hour, FIRST_HOUR);

        // Live games have no loop.
        reset_storage();
        set_env("alice.testnet", FIRST_HOUR * 3600 + 10, 0);
        let mut cfg = test_config();
        cfg.replay = false;
        cfg.replay_hours = 0;
        let live = Contract::new(cfg);
        assert_eq!(live.cfg.hour_for_round(5), FIRST_HOUR + 5);
        assert_eq!(live.cfg.hour_for_round(400), FIRST_HOUR + 400);
    }

    #[test]
    fn config_validation_checks_replay_hours() {
        let mut cfg = test_config();
        cfg.replay_hours = 0;
        assert!(std::panic::catch_unwind(|| cfg.validate()).is_err());

        let mut cfg = test_config();
        cfg.replay = false;
        assert!(std::panic::catch_unwind(|| cfg.validate()).is_err());

        let mut cfg = test_config();
        cfg.replay = false;
        cfg.replay_hours = 0;
        set_env("alice.testnet", FIRST_HOUR * 3600 + 10, 0);
        cfg.validate();

        // 100% evaporation would zero a pool while its shares still exist.
        let mut cfg = test_config();
        cfg.evap_bps = 10_000;
        assert!(std::panic::catch_unwind(|| cfg.validate()).is_err());

        // MIN_PLANT / MIN_SHARES_OUT assume 18 decimals.
        let mut cfg = test_config();
        cfg.decimals = 17;
        assert!(std::panic::catch_unwind(|| cfg.validate()).is_err());

        let mut cfg = test_config();
        cfg.emission_per_round = U128(0);
        assert!(std::panic::catch_unwind(|| cfg.validate()).is_err());

        let mut cfg = test_config();
        cfg.oracle.deposit = U128(0);
        assert!(std::panic::catch_unwind(|| cfg.validate()).is_err());

        let mut cfg = test_config();
        cfg.seed_amount = U128(MIN_PLANT - 1);
        assert!(std::panic::catch_unwind(|| cfg.validate()).is_err());

        // Caps: 1M DRIZZLE per mint field.
        let cap = 10u128.pow(24);
        let mut cfg = test_config();
        cfg.emission_per_round = U128(cap + 1);
        assert!(std::panic::catch_unwind(|| cfg.validate()).is_err());
        let mut cfg = test_config();
        cfg.seed_amount = U128(cap + 1);
        assert!(std::panic::catch_unwind(|| cfg.validate()).is_err());
        let mut cfg = test_config();
        cfg.keeper_tip = U128(cap + 1);
        assert!(std::panic::catch_unwind(|| cfg.validate()).is_err());
        let mut cfg = test_config();
        cfg.emission_per_round = U128(cap);
        cfg.validate();

        // A live game must start in the current hour (+/- 1).
        set_env("alice.testnet", FIRST_HOUR * 3600 + 10, 0);
        for delta in [0u64, 1] {
            let mut cfg = test_config();
            cfg.replay = false;
            cfg.replay_hours = 0;
            cfg.first_hour = FIRST_HOUR + delta;
            cfg.validate();
        }
        for delta in [2u64, 100] {
            let mut cfg = test_config();
            cfg.replay = false;
            cfg.replay_hours = 0;
            cfg.first_hour = FIRST_HOUR + delta;
            assert!(std::panic::catch_unwind(|| cfg.validate()).is_err());
        }
    }

    #[test]
    fn on_weather_rejects_the_unwrapped_hour_at_the_loop_boundary() {
        let mut c = setup();
        c.next_round = 168;
        let due = GENESIS + 169 * ROUND_SECS;
        set_env("alice.testnet", due, ORACLE_DEPOSIT);
        let _ = c.tick();

        // Round 168 maps back to FIRST_HOUR, so FIRST_HOUR + 168 is wrong.
        let bytes = near_sdk::serde_json::to_vec(&json!({
            "hour": FIRST_HOUR + 168,
            "mm_tenths": zero_mm().to_vec(),
            "source": "open-meteo",
        }))
        .unwrap();
        callback_env(due + 5, PromiseResult::Successful(bytes));
        let result = callback_result();
        c.on_weather(168, acct("alice.testnet"), result);
        assert_eq!(c.next_round, 168, "a wrong hour must not settle the round");
        assert!(c.in_flight.is_none());
        assert!(c.get_round_log(168).is_none());

        // The looped hour settles normally.
        settle(&mut c, &zero_mm(), due + ROUND_SECS, "alice.testnet");
        assert_eq!(c.next_round, 169);
        assert_eq!(c.get_round_log(168).unwrap().hour, FIRST_HOUR);
    }

    #[test]
    fn round_logs_are_pruned_to_the_retention_window() {
        let mut c = setup();
        for round in 0..(ROUND_LOG_RETENTION + 5) {
            settle(
                &mut c,
                &zero_mm(),
                GENESIS + ROUND_SECS * (round + 1),
                "alice.testnet",
            );
        }
        assert_eq!(c.next_round, ROUND_LOG_RETENTION + 5);
        let state = c.get_round_state();
        assert_eq!(state.oldest_round_log, 5);
        for round in 0..5 {
            assert!(
                c.get_round_log(round).is_none(),
                "round {round} should be pruned"
            );
        }
        assert!(c.get_round_log(5).is_some());
        assert!(c.get_round_log(ROUND_LOG_RETENTION + 4).is_some());
        // Pruned rounds are skipped by the range query.
        let from_zero = c.get_round_logs(0, 100);
        assert_eq!(from_zero.len(), 95);
        assert_eq!(from_zero[0].round, 5);
        let stored = c.get_round_logs(5, 100);
        assert_eq!(stored.len(), 100);
        assert_eq!(stored[0].round, 5);
        assert_eq!(stored[99].round, 104);

        // One full RoundLog: 12 cities, 4 U128 arrays, keeper, request id.
        let log = c.get_round_log(ROUND_LOG_RETENTION + 4).unwrap();
        let size = near_sdk::borsh::to_vec(&log).unwrap().len();
        let cost = env::storage_byte_cost().as_yoctonear() * size as u128;
        println!(
            "RoundLog borsh size: {size} bytes; storage stake: {cost} yocto ({:.6} NEAR)",
            cost as f64 / 1e24
        );
        assert!(size > 200 && size < 2_000, "unexpected RoundLog size {size}");
        assert!(cost < 20_000_000_000_000_000_000_000, "log too expensive");
    }

    fn panic_message(err: Box<dyn std::any::Any + Send>) -> String {
        if let Some(message) = err.downcast_ref::<String>() {
            message.clone()
        } else if let Some(message) = err.downcast_ref::<&str>() {
            (*message).to_string()
        } else {
            String::new()
        }
    }

    // ------------------------------------------------------------ F1: share price

    #[test]
    fn plant_minimum_and_worst_price_guard_reject_dust() {
        let mut c = setup();
        seed(&mut c, "alice.testnet", GENESIS);
        seed(&mut c, "bob.testnet", GENESIS);

        // 1 yocto is far below MIN_PLANT.
        set_env("bob.testnet", GENESIS + 1, 1);
        let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.plant(0, U128(1));
        }))
        .unwrap_err();
        assert!(panic_message(err).contains("1 DRIZZLE minimum"));

        // Craft a pool whose shares are worth far more than a 1 DRIZZLE plant.
        plant(&mut c, "alice.testnet", 0, DEC, GENESIS + 2);
        settle(&mut c, &zero_mm(), GENESIS + ROUND_SECS, "alice.testnet");
        c.cities[0].value = 10u128.pow(37);

        set_env("bob.testnet", GENESIS + 70, 1);
        let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.plant(0, U128(DEC));
        }))
        .unwrap_err();
        let message = panic_message(err);
        assert!(message.contains("share price too high"), "{message}");
        let shares = c.cities[0].shares;
        assert!(message.contains(&format!(
            "plant at least {}",
            pool::mul_div(MIN_SHARES_OUT, 10u128.pow(37) + 100 * DEC, shares) + 1
        )));
    }

    #[test]
    fn high_price_pool_does_not_confiscate_a_new_planter() {
        let mut c = setup();
        let u = users();
        seed(&mut c, "alice.testnet", GENESIS);
        seed(&mut c, "bob.testnet", GENESIS);

        // The attacker's minimum plant becomes 1 share of the pool.
        plant(&mut c, "alice.testnet", 0, DEC, GENESIS + 1);
        settle(&mut c, &zero_mm(), GENESIS + ROUND_SECS, "alice.testnet");
        let mut mm = zero_mm();
        mm[0] = 10;
        settle(&mut c, &mm, GENESIS + 2 * ROUND_SECS, "alice.testnet");
        assert_eq!(c.cities[0].value, 101 * DEC);

        // The victim's 50 DRIZZLE must not be rounded to zero shares.
        plant(&mut c, "bob.testnet", 0, 50 * DEC, GENESIS + 130);
        settle(&mut c, &zero_mm(), GENESIS + 3 * ROUND_SECS, "alice.testnet");
        let got = uproot(&mut c, "bob.testnet", 0, None, GENESIS + 200);
        assert!(
            got * 1000 >= 50 * DEC * 999,
            "victim recovered {got} of {}",
            50 * DEC
        );
        assert_invariants(&c, &u);
    }

    #[test]
    fn partial_uproot_that_leaves_dust_is_rejected() {
        let mut c = setup();
        seed(&mut c, "alice.testnet", GENESIS);
        plant(&mut c, "alice.testnet", 0, DEC, GENESIS + 1);
        settle(&mut c, &zero_mm(), GENESIS + ROUND_SECS, "alice.testnet");
        // Craft a near-worthless pool (1 000 yocto over 1e18 shares).
        c.cities[0].value = 1_000;

        set_env("alice.testnet", GENESIS + 70, 1);
        let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.uproot(0, Some(U128(500_000_000_000_000_000)));
        }))
        .unwrap_err();
        assert!(panic_message(err).contains("remaining stake below minimum"));
    }

    // ------------------------------------------------------------ F2: planters

    #[test]
    fn planter_counter_does_not_drift_on_zero_share_sprout() {
        let mut c = setup();
        let u = users();
        seed(&mut c, "alice.testnet", GENESIS);
        seed(&mut c, "bob.testnet", GENESIS);

        plant(&mut c, "alice.testnet", 0, DEC, GENESIS + 1);
        settle(&mut c, &zero_mm(), GENESIS + ROUND_SECS, "alice.testnet");
        assert_eq!(planters(&c, 0), 1);

        // Craft bob's 1-yocto pending for round 1; at ~101 DRIZZLE/share it
        // sprouts to exactly zero shares.
        let bob = acct("bob.testnet");
        let contract = acct(CONTRACT_ID);
        c.ft.internal_transfer(&bob, &contract, 1, None);
        c.cities[0].pending_total += 1;
        c.cities[0].planters += 1;
        c.users.insert(
            &(bob.clone(), 0),
            &UserCity {
                shares: 0,
                pending: 1,
                pending_round: 1,
                epoch: 0,
            },
        );
        c.bucket_add_holder(1, 0);

        let mut mm = zero_mm();
        mm[0] = 10;
        settle(&mut c, &mm, GENESIS + 2 * ROUND_SECS, "alice.testnet");
        assert_eq!(c.buckets.get(&(1, 0)).unwrap().ratio, Some((1, 0)));

        let got = uproot(&mut c, "bob.testnet", 0, None, GENESIS + 130);
        assert_eq!(got, 0, "the dust sprout was worth zero shares");
        assert_eq!(
            planters(&c, 0),
            1,
            "the zero-share planter must be decremented"
        );

        uproot(&mut c, "alice.testnet", 0, None, GENESIS + 131);
        assert_eq!(planters(&c, 0), 0, "the last-holder sweep must be reachable");
        assert_eq!(c.cities[0].value, 0);
        assert_invariants(&c, &u);
    }

    // ------------------------------------------------------------ F3: buckets

    #[test]
    fn bucket_map_is_emptied_after_plant_sprout_cycles() {
        let mut c = setup();
        seed(&mut c, "alice.testnet", GENESIS);
        for round in 0..50u64 {
            plant(&mut c, "alice.testnet", 0, DEC, GENESIS + 1 + round);
            settle(
                &mut c,
                &zero_mm(),
                GENESIS + ROUND_SECS * (round + 1),
                "alice.testnet",
            );
        }
        // Each plant released the previous round's bucket.
        for round in 0..49u64 {
            assert!(
                c.buckets.get(&(round, 0)).is_none(),
                "bucket for round {round} leaked"
            );
        }
        // The last one is released when the position sprouts on uproot.
        uproot(&mut c, "alice.testnet", 0, None, GENESIS + 100);
        assert!(c.buckets.get(&(49, 0)).is_none(), "last bucket leaked");
    }

    // ------------------------------------------------------------ F4: epochs

    #[test]
    fn dead_pool_reset_burns_dust_and_retires_old_shares() {
        let mut c = setup();
        let u = users();
        seed(&mut c, "alice.testnet", GENESIS);
        seed(&mut c, "bob.testnet", GENESIS);

        plant(&mut c, "alice.testnet", 0, DEC, GENESIS + 1);
        // Round 0: zero shares at payout, so the minimum plant just sprouts.
        settle(&mut c, &zero_mm(), GENESIS + ROUND_SECS, "alice.testnet");
        // Round 1: the only rainy city, so all 100 DRIZZLE go to the pool.
        let mut mm = zero_mm();
        mm[0] = 10;
        settle(&mut c, &mm, GENESIS + 2 * ROUND_SECS, "alice.testnet");
        assert_eq!(c.cities[0].value, 101 * DEC);

        // Simulate a long dry spell: burn the pool down to dust.
        let contract = acct(CONTRACT_ID);
        let drain = c.cities[0].value - 500;
        c.ft.internal_withdraw(&contract, drain);
        c.burned += drain;
        c.cities[0].value = 500;
        assert_invariants(&c, &u);

        plant(&mut c, "bob.testnet", 0, DEC, GENESIS + 130);
        settle(&mut c, &zero_mm(), GENESIS + 3 * ROUND_SECS, "alice.testnet");
        assert_invariants(&c, &u);

        // Round 0 was the first (empty-pool) reset, round 2 the dust reset.
        assert_eq!(c.cities[0].epoch, 2);
        assert_eq!(c.cities[0].shares, DEC + pool::DEAD_SHARES);
        assert_eq!(c.cities[0].value, DEC);
        assert_eq!(planters(&c, 0), 1, "only the new-epoch holder counts");
        assert_eq!(c.get_round_log(2).unwrap().burned[0].0, 500);

        // The old holder's shares are worthless, but uprooting them is harmless.
        assert_eq!(c.get_user(acct("alice.testnet")).cities[0].shares.0, 0);
        assert_eq!(
            uproot(&mut c, "alice.testnet", 0, None, GENESIS + 130),
            0
        );
        assert_eq!(planters(&c, 0), 1);

        // The new planter sprouted 1:1 at the reset.
        let bob = &c.get_user(acct("bob.testnet")).cities[0];
        assert_eq!(bob.shares.0, DEC);
        assert!(
            bob.value.0 <= DEC && bob.value.0 + 2 * pool::DEAD_SHARES >= DEC,
            "reset sprout is ~1:1 (got {})",
            bob.value.0
        );

        assert_eq!(uproot(&mut c, "bob.testnet", 0, None, GENESIS + 131), DEC);
        assert_eq!(planters(&c, 0), 0);
        assert_eq!(c.cities[0].value, 0);
        assert_invariants(&c, &u);
    }

    // ------------------------------------------------------------ S1: bucket release

    #[test]
    fn uproot_before_settlement_releases_the_bucket() {
        let mut c = setup();
        seed(&mut c, "alice.testnet", GENESIS);
        plant(&mut c, "alice.testnet", 0, DEC, GENESIS + 1);
        assert_eq!(c.buckets.get(&(0, 0)).unwrap().holders, 1);

        assert_eq!(uproot(&mut c, "alice.testnet", 0, None, GENESIS + 2), DEC);
        assert!(c.buckets.get(&(0, 0)).is_none(), "cancelled plant leaked a bucket");

        // Even after unregistering: the bucket is gone and `seeded` is retained.
        set_env("alice.testnet", GENESIS + 3, 1);
        assert!(c.storage_unregister(Some(true)));
        assert!(c.seeded.contains(&acct("alice.testnet")));
        assert!(c.buckets.get(&(0, 0)).is_none());
    }

    #[test]
    fn uproot_none_keeps_a_bucket_shared_with_another_planter() {
        let mut c = setup();
        let u = users();
        seed(&mut c, "alice.testnet", GENESIS);
        seed(&mut c, "bob.testnet", GENESIS);
        plant(&mut c, "alice.testnet", 0, DEC, GENESIS + 1);
        plant(&mut c, "bob.testnet", 0, DEC, GENESIS + 2);
        assert_eq!(c.buckets.get(&(0, 0)).unwrap().holders, 2);

        assert_eq!(uproot(&mut c, "alice.testnet", 0, None, GENESIS + 3), DEC);
        assert_eq!(c.buckets.get(&(0, 0)).unwrap().holders, 1);

        settle(&mut c, &zero_mm(), GENESIS + ROUND_SECS, "alice.testnet");
        let bucket = c.buckets.get(&(0, 0)).unwrap();
        assert_eq!(bucket.ratio, Some((DEC, DEC)));
        assert_eq!(bucket.holders, 1);

        // Bob still sprouted correctly despite the shared bucket.
        assert_eq!(uproot(&mut c, "bob.testnet", 0, None, GENESIS + 70), DEC);
        assert!(c.buckets.get(&(0, 0)).is_none());
        assert_invariants(&c, &u);
    }

    // ------------------------------------------------------------ S2: live freeze

    #[test]
    fn live_mode_freezes_plant_and_uproot_after_the_hour_closes() {
        reset_storage();
        set_env("alice.testnet", 500_000 * 3600 + 10, 0);
        let mut cfg = test_config();
        cfg.replay = false;
        cfg.replay_hours = 0;
        cfg.first_hour = 500_000;
        let mut c = Contract::new(cfg);
        let close = (500_000 + 1) * 3600;

        let min = c.storage_balance_bounds().min.as_yoctonear();
        set_env("alice.testnet", close - 1, min);
        c.claim_seed();
        set_env("alice.testnet", close - 1, 1);
        c.plant(0, U128(DEC));

        set_env("alice.testnet", close, 1);
        let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.plant(1, U128(DEC));
        }))
        .unwrap_err();
        assert!(panic_message(err).contains("round closing"));

        set_env("alice.testnet", close, 1);
        let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.uproot(0, None);
        }))
        .unwrap_err();
        assert!(panic_message(err).contains("round closing"));

        // Replay is exempt at the same wall clock.
        let mut r = setup();
        seed(&mut r, "alice.testnet", close);
        plant(&mut r, "alice.testnet", 0, DEC, close);
        assert_eq!(r.cities[0].pending_total, DEC);
    }

    // ------------------------------------------------------------ R1: dead shares

    #[test]
    fn sole_holder_cannot_lock_the_city_out_of_new_planters() {
        let mut c = setup();
        let u = users();
        seed(&mut c, "alice.testnet", GENESIS);
        seed(&mut c, "bob.testnet", GENESIS);
        plant(&mut c, "alice.testnet", 0, 10 * DEC, GENESIS + 1);

        let mut mm = zero_mm();
        mm[0] = 10;
        for round in 0..20u64 {
            settle(
                &mut c,
                &mm,
                GENESIS + ROUND_SECS * (round + 1),
                "alice.testnet",
            );
            // Shrink the position to ~1 DRIZZLE of value, then self-dance.
            let pool = c.cities[0].to_pool();
            let keep = pool::mul_div(MIN_PLANT, pool.shares, pool.value) + 2;
            let held = c.get_user(acct("alice.testnet")).cities[0].shares.0;
            if keep < held {
                uproot(
                    &mut c,
                    "alice.testnet",
                    0,
                    Some(held - keep),
                    GENESIS + ROUND_SECS * (round + 1) + 1,
                );
            }
            rain_dance(
                &mut c,
                "alice.testnet",
                0,
                DEC,
                GENESIS + ROUND_SECS * (round + 1) + 2,
            );
            assert_invariants(&c, &u);
        }

        // The monopolist could not inflate the price past a 1 DRIZZLE plant.
        let before = c.cities[0].pending_total;
        plant(&mut c, "bob.testnet", 0, DEC, GENESIS + 2_000);
        assert_eq!(c.cities[0].pending_total, before + DEC);
        settle(&mut c, &mm, GENESIS + 2_100, "alice.testnet");
        let bob = &c.get_user(acct("bob.testnet")).cities[0];
        assert!(bob.shares.0 > 0, "the newcomer must receive shares");
        assert_invariants(&c, &u);
    }

    // ------------------------------------------------------------ R2: exit hatch

    #[test]
    fn live_exit_is_allowed_after_one_day_of_missing_settlement() {
        reset_storage();
        set_env("alice.testnet", 500_000 * 3600 + 10, 0);
        let mut cfg = test_config();
        cfg.replay = false;
        cfg.replay_hours = 0;
        cfg.first_hour = 500_000;
        let mut c = Contract::new(cfg);
        let close = (500_000 + 1) * 3600;

        let min = c.storage_balance_bounds().min.as_yoctonear();
        set_env("alice.testnet", close - 1, min);
        c.claim_seed();
        set_env("alice.testnet", close - 1, 1);
        c.plant(0, U128(DEC));

        // One second too early: still frozen.
        set_env("alice.testnet", close + 86_399, 1);
        let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.uproot(0, None);
        }))
        .unwrap_err();
        assert!(panic_message(err).contains("round closing"));

        // Partial uproots stay frozen even after the day.
        set_env("alice.testnet", close + 86_400, 1);
        let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.uproot(0, Some(U128(1)));
        }))
        .unwrap_err();
        assert!(panic_message(err).contains("round closing"));

        // Planting stays frozen.
        set_env("alice.testnet", close + 86_400, 1);
        let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.plant(1, U128(DEC));
        }))
        .unwrap_err();
        assert!(panic_message(err).contains("round closing"));

        // The full exit works.
        set_env("alice.testnet", close + 86_400, 1);
        assert_eq!(c.uproot(0, None).0, DEC);
    }

    // ------------------------------------------------------------ R4: discount

    #[test]
    fn reregistration_does_not_recharge_the_seeded_entry() {
        let mut c = setup();
        let alice = acct("alice.testnet");
        let full = c.storage_min().as_yoctonear();
        let seeded_cost = env::storage_byte_cost().as_yoctonear() * c.seeded_storage as u128;
        assert!(seeded_cost > 0);

        seed(&mut c, "alice.testnet", GENESIS);
        set_env("alice.testnet", GENESIS + 1, 1);
        assert!(c.storage_unregister(Some(true)));
        assert!(c.seeded.contains(&alice));

        // Bounds stay at the full bond, but the charge skips the retained entry.
        assert_eq!(c.storage_balance_bounds().min.as_yoctonear(), full);
        assert_eq!(c.registration_min(&alice).as_yoctonear(), full - seeded_cost);

        set_env("alice.testnet", GENESIS + 2, full - seeded_cost - 1);
        let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.storage_deposit(None, None);
        }))
        .unwrap_err();
        assert!(panic_message(err).contains("minimum storage balance"));

        set_env("alice.testnet", GENESIS + 2, full - seeded_cost);
        let balance = c.storage_deposit(None, None);
        assert_eq!(balance.total.as_yoctonear(), full);
        assert!(c.storage_balance_of(alice.clone()).is_some());

        set_env("alice.testnet", GENESIS + 3, 1);
        assert!(c.storage_unregister(Some(true)));
        assert!(c.seeded.contains(&alice));
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
        assert_eq!(value["replay_hours"], REPLAY_HOURS);
        let state = near_sdk::serde_json::to_value(c.get_round_state()).unwrap();
        assert_eq!(state["next_round"], 0);
        assert_eq!(state["due_at"], GENESIS + ROUND_SECS);
        assert_eq!(state["oldest_round_log"], 0);
    }
}
