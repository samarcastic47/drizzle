//! Immutable contract configuration, stored in state and returned by `get_config`.
//!
//! The JSON shapes here are part of `docs/interface.md`: the outlayer code source
//! serializes as `{"WasmUrl":{...}}` (externally tagged), while `resource_limits`
//! is a plain object.

use near_sdk::json_types::U128;
use near_sdk::{near, AccountId};

/// A city the game watches. Index in the config vector is the city id.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug)]
pub struct City {
    pub name: String,
    pub lat_e4: i32,
    pub lon_e4: i32,
}

/// A WASM file pinned by sha256, as understood by OutLayer's `WasmUrl` source.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug)]
pub struct WasmUrlSource {
    pub url: String,
    pub hash: String,
    pub build_target: String,
}

/// OutLayer's `ExecutionSource` (externally tagged, single variant we use).
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug)]
pub enum ExecutionSource {
    WasmUrl(WasmUrlSource),
}

impl ExecutionSource {
    pub fn wasm(&self) -> &WasmUrlSource {
        match self {
            ExecutionSource::WasmUrl(s) => s,
        }
    }
}

/// OutLayer's `ResourceLimits` (plain object on the wire).
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug)]
pub struct ResourceLimits {
    pub max_instructions: u64,
    pub max_memory_mb: u32,
    pub max_execution_seconds: u64,
}

/// Everything needed to call `request_execution` on `outlayer`.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug)]
pub struct OracleConfig {
    pub outlayer: AccountId,
    pub source: ExecutionSource,
    pub resource_limits: ResourceLimits,
    /// Deposit attached to every `request_execution`, in yoctoNEAR.
    pub deposit: U128,
}

/// All contract parameters. Set once in `new`, never mutable.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug)]
pub struct Config {
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub icon: Option<String>,

    pub cities: Vec<City>,

    pub emission_per_round: U128,
    pub evap_bps: u16,
    pub rain_threshold_tenths: u16,
    pub cap_tenths: u16,
    pub seed_amount: U128,
    pub keeper_tip: U128,

    pub round_seconds: u64,
    pub genesis_ts: u64,
    pub first_hour: u64,
    pub replay: bool,
    /// Replay only: the real hours are replayed in a loop of this length.
    /// `0` for live games (and then round `r` uses hour `first_hour + r`).
    pub replay_hours: u64,

    pub oracle: OracleConfig,
}

/// Number of cities in every game. Fixed by the mechanic (one Open-Meteo call).
pub const NUM_CITIES: usize = 12;

impl Config {
    /// Panics on a config that would break the game's invariants.
    pub fn validate(&self) {
        assert_eq!(
            self.cities.len(),
            NUM_CITIES,
            "config: exactly {NUM_CITIES} cities required"
        );
        assert!(self.evap_bps < 10_000, "config: evap_bps must be < 10000");
        assert!(
            self.rain_threshold_tenths <= self.cap_tenths,
            "config: rain threshold must be <= storm cap"
        );
        assert!(self.round_seconds > 0, "config: round_seconds must be > 0");
        assert!(self.decimals > 0, "config: decimals must be > 0");
        if self.replay {
            assert!(
                self.replay_hours > 0,
                "config: replay games need replay_hours > 0"
            );
        } else {
            assert_eq!(
                self.replay_hours, 0,
                "config: live games need replay_hours == 0"
            );
        }
        for (i, c) in self.cities.iter().enumerate() {
            assert!(
                (-900_000..=900_000).contains(&c.lat_e4),
                "config: city {i} latitude out of range"
            );
            assert!(
                (-1_800_000..=1_800_000).contains(&c.lon_e4),
                "config: city {i} longitude out of range"
            );
        }
    }

    /// The real unix hour that round `round` observes.
    ///
    /// Replay loops over the first `replay_hours` historical hours so the game can
    /// run forever (Open-Meteo has no data for future hours); live games advance
    /// one real hour per round.
    pub fn hour_for_round(&self, round: u64) -> u64 {
        let offset = if self.replay_hours > 0 {
            round % self.replay_hours
        } else {
            round
        };
        self.first_hour + offset
    }

    /// Parameters for the pure pool math.
    pub fn pool_params(&self) -> crate::pool::Params {
        crate::pool::Params {
            emission: self.emission_per_round.0,
            evap_bps: self.evap_bps,
            threshold_tenths: self.rain_threshold_tenths,
            cap_tenths: self.cap_tenths,
        }
    }
}
