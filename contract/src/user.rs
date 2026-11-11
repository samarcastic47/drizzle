//! Persisted per-city and per-user state.
//!
//! `pool::Pool` is deliberately plain Rust (no borsh), so the contract stores its
//! own mirrored `CityState` and converts at the boundary.

use crate::pool::Pool;
use near_sdk::near;

/// A city's persisted pool. Mirrors `pool::Pool` plus the planter counter.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, Default)]
pub struct CityState {
    pub value: u128,
    pub shares: u128,
    pub pending_total: u128,
    pub jackpot: u128,
    pub last_rain_round: Option<u64>,
    pub last_mm_tenths: u16,
    /// Accounts with `shares > 0 || pending > 0` in this city.
    pub planters: u32,
}

impl CityState {
    pub fn to_pool(&self) -> Pool {
        Pool {
            value: self.value,
            shares: self.shares,
            pending_total: self.pending_total,
            jackpot: self.jackpot,
            last_rain_round: self.last_rain_round,
            last_mm_tenths: self.last_mm_tenths,
        }
    }

    pub fn absorb(&mut self, pool: &Pool) {
        self.value = pool.value;
        self.shares = pool.shares;
        self.pending_total = pool.pending_total;
        self.jackpot = pool.jackpot;
        self.last_rain_round = pool.last_rain_round;
        self.last_mm_tenths = pool.last_mm_tenths;
    }

    /// Whether this account counts as a planter here.
    pub fn is_planted(&self) -> bool {
        self.shares > 0 || self.pending_total > 0
    }
}

/// One account's position in one city.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, Default)]
pub struct UserCity {
    pub shares: u128,
    /// Tokens planted and not yet converted to shares.
    pub pending: u128,
    /// Round whose settlement will sprout `pending` (only meaningful when pending > 0).
    pub pending_round: u64,
}

impl UserCity {
    pub fn is_empty(&self) -> bool {
        self.shares == 0 && self.pending == 0
    }
}
