//! View structs and the stored round log. Shapes are fixed by `docs/interface.md`.

use near_sdk::json_types::U128;
use near_sdk::{near, AccountId};

/// Stored per settled round (`get_round_log`, `get_round_logs`).
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug)]
pub struct RoundLog {
    pub round: u64,
    /// Unix hour this round covers (`first_hour + round`).
    pub hour: u64,
    pub mm_tenths: Vec<u16>,
    /// Pool mints per city, jackpot payout included.
    pub minted: Vec<U128>,
    /// Evaporated value per city.
    pub burned: Vec<U128>,
    /// Jackpot paid out per city (0 where the city did not rain or had no planters).
    pub jackpot_paid: Vec<U128>,
    /// Pending tokens converted to shares per city.
    pub sprouted: Vec<U128>,
    pub keeper: AccountId,
    pub keeper_tip: U128,
    pub outlayer_request_id: Option<String>,
    pub tick_block_height: u64,
    pub settled_ts: u64,
}

/// `get_round_state().in_flight`
#[near(serializers = [json])]
#[derive(Clone, Debug)]
pub struct InFlightView {
    pub round: u64,
    pub started_ts: u64,
}

/// `get_round_state()`
#[near(serializers = [json])]
#[derive(Clone, Debug)]
pub struct RoundStateView {
    pub next_round: u64,
    pub next_hour: u64,
    pub due_at: u64,
    pub in_flight: Option<InFlightView>,
    pub now: u64,
}

/// `get_cities()` entry.
#[near(serializers = [json])]
#[derive(Clone, Debug)]
pub struct CityView {
    pub index: u8,
    pub name: String,
    pub lat_e4: i32,
    pub lon_e4: i32,
    pub value: U128,
    pub shares: U128,
    pub pending_total: U128,
    pub jackpot: U128,
    pub planters: u32,
    pub last_rain_round: Option<u64>,
    pub last_mm_tenths: u16,
}

/// `get_user().cities[i]`
#[near(serializers = [json])]
#[derive(Clone, Debug)]
pub struct UserCityView {
    pub index: u8,
    pub shares: U128,
    pub value: U128,
    pub pending: U128,
    pub pending_round: Option<u64>,
}

/// `get_user()`
#[near(serializers = [json])]
#[derive(Clone, Debug)]
pub struct UserView {
    pub balance: U128,
    pub seeded: bool,
    pub cities: Vec<UserCityView>,
}

/// `get_stats()`
#[near(serializers = [json])]
#[derive(Clone, Debug)]
pub struct StatsView {
    pub total_supply: U128,
    pub minted: U128,
    pub burned: U128,
    pub danced: U128,
    pub seeded_count: u64,
    pub rounds_settled: u64,
}
