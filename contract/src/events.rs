//! Custom NEP-297 events (`standard = "drizzle"`, `version = "1.0.0"`).
//!
//! Shapes are fixed by `docs/interface.md` §Events.

use near_sdk::json_types::U128;
use near_sdk::{near, AccountId};
use near_sdk::serde::Serialize;

/// `drizzle` `round` payload.
#[derive(Clone, Debug, Serialize)]
#[serde(crate = "near_sdk::serde")]
pub struct RoundEvent {
    pub round: u64,
    pub hour: u64,
    pub mm_tenths: Vec<u16>,
    pub minted: Vec<U128>,
    pub burned: Vec<U128>,
    pub keeper: AccountId,
}

/// `drizzle` `round_failed` payload.
#[derive(Clone, Debug, Serialize)]
#[serde(crate = "near_sdk::serde")]
pub struct RoundFailedEvent {
    pub round: u64,
    pub reason: String,
}

/// `drizzle` `plant` payload.
#[derive(Clone, Debug, Serialize)]
#[serde(crate = "near_sdk::serde")]
pub struct PlantEvent {
    pub account_id: AccountId,
    pub city: u8,
    pub amount: U128,
    pub sprouts_after_round: u64,
}

/// `drizzle` `uproot` payload.
#[derive(Clone, Debug, Serialize)]
#[serde(crate = "near_sdk::serde")]
pub struct UprootEvent {
    pub account_id: AccountId,
    pub city: u8,
    pub shares: U128,
    pub amount: U128,
}

/// `drizzle` `rain_dance` payload.
#[derive(Clone, Debug, Serialize)]
#[serde(crate = "near_sdk::serde")]
pub struct RainDanceEvent {
    pub account_id: AccountId,
    pub city: u8,
    pub amount: U128,
    pub jackpot: U128,
}

/// `drizzle` `seed` payload.
#[derive(Clone, Debug, Serialize)]
#[serde(crate = "near_sdk::serde")]
pub struct SeedEvent {
    pub account_id: AccountId,
    pub amount: U128,
}

/// All custom events. `data` is always a one-element array (NEP-297 flexible data).
#[near(event_json(standard = "drizzle"))]
pub enum DrizzleEvents {
    #[event_version("1.0.0")]
    Round(Vec<RoundEvent>),
    #[event_version("1.0.0")]
    RoundFailed(Vec<RoundFailedEvent>),
    #[event_version("1.0.0")]
    Plant(Vec<PlantEvent>),
    #[event_version("1.0.0")]
    Uproot(Vec<UprootEvent>),
    #[event_version("1.0.0")]
    RainDance(Vec<RainDanceEvent>),
    #[event_version("1.0.0")]
    Seed(Vec<SeedEvent>),
}
