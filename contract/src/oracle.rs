//! OutLayer (`outlayer.testnet`) integration: the cross-contract call, the oracle
//! input builder and defensive parsing of the callback payload.
//!
//! Live ABI and callback semantics are documented in `docs/notes/outlayer.md` §1–2.

use crate::config::{City, ExecutionSource, ResourceLimits};
use near_sdk::serde::Deserialize;
use near_sdk::serde_json::{json, Value};
use near_sdk::{ext_contract, AccountId, Gas, Promise};

/// Gas reserved for OutLayer's `request_execution` (it reserves >= 50 Tgas internally).
pub const OUTLAYER_GAS: Gas = Gas::from_tgas(200);
/// Gas for our `on_weather` callback: 12 mints/burns + 12 sprout-ratio writes + event.
pub const WEATHER_CALLBACK_GAS: Gas = Gas::from_tgas(60);
/// A stuck OutLayer request never resumes, so the in-flight lock self-expires.
pub const IN_FLIGHT_TIMEOUT_SECS: u64 = 600;

/// The seven-argument live `request_execution` ABI.
#[ext_contract(ext_outlayer)]
pub trait OutLayer {
    fn request_execution(
        &mut self,
        source: Value,
        resource_limits: Option<Value>,
        input_data: Option<String>,
        secrets_ref: Option<Value>,
        response_format: Option<String>,
        payer_account_id: Option<AccountId>,
        params: Option<Value>,
    );
}

/// Calls into the contract itself for the settlement callback.
#[ext_contract(ext_self)]
pub trait DrizzleSelf {
    fn on_weather(&mut self, round: u64, keeper: AccountId);
}

/// The oracle's stdout, e.g.
/// `{"hour":497294,"mm_tenths":[0,0,3,...],"source":"open-meteo","outlayer_request_id":"22023"}`.
#[derive(Clone, Debug, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct WeatherPayload {
    pub hour: u64,
    pub mm_tenths: Vec<u16>,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub outlayer_request_id: Option<String>,
}

/// stdin for the WASI oracle: `{"hour":H,"cities":[[lat_e4,lon_e4],...]}`
pub fn build_input(hour: u64, cities: &[City]) -> String {
    let coords: Vec<[i32; 2]> = cities.iter().map(|c| [c.lat_e4, c.lon_e4]).collect();
    near_sdk::serde_json::to_string(&json!({ "hour": hour, "cities": coords }))
        .unwrap_or_else(|_| near_sdk::env::abort())
}

/// Accepts both the live raw payload and a stale `OutLayerResponse {success,result,error}`
/// wrapper (docs/notes/outlayer.md §2.2: the wrapped form is not what the chain returns,
/// but tolerating it costs one map lookup).
pub fn unwrap_response(value: Value) -> Result<Value, String> {
    if let Value::Object(obj) = &value {
        if obj.contains_key("success") && (obj.contains_key("result") || obj.contains_key("error")) {
            if obj.get("success") != Some(&Value::Bool(true)) {
                let err = obj
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error")
                    .to_string();
                return Err(format!("outlayer error: {err}"));
            }
            return match obj.get("result") {
                Some(Value::Null) | None => Err("outlayer returned no result".to_string()),
                Some(result) => Ok(result.clone()),
            };
        }
    }
    Ok(value)
}

/// Parses and fully validates the oracle payload for `expected_hour` / `n_cities`.
pub fn parse_reading(
    value: Value,
    expected_hour: u64,
    n_cities: usize,
) -> Result<WeatherPayload, String> {
    let value = unwrap_response(value)?;
    let reading: WeatherPayload = near_sdk::serde_json::from_value(value)
        .map_err(|e| format!("malformed oracle payload: {e}"))?;
    if reading.hour != expected_hour {
        return Err(format!(
            "wrong hour: got {}, expected {}",
            reading.hour, expected_hour
        ));
    }
    if reading.mm_tenths.len() != n_cities {
        return Err(format!(
            "wrong mm_tenths length: got {}, expected {}",
            reading.mm_tenths.len(),
            n_cities
        ));
    }
    Ok(reading)
}

/// Serializes the pinned source / limits for the call.
pub fn source_json(source: &ExecutionSource) -> Value {
    near_sdk::serde_json::to_value(source).unwrap_or_else(|_| near_sdk::env::abort())
}

pub fn limits_json(limits: &ResourceLimits) -> Value {
    near_sdk::serde_json::to_value(limits).unwrap_or_else(|_| near_sdk::env::abort())
}

/// Builds the OutLayer promise (not `.then`-ed; the caller chains the callback).
#[allow(clippy::too_many_arguments)]
pub fn request_promise(
    outlayer: AccountId,
    source: &ExecutionSource,
    limits: &ResourceLimits,
    input: String,
    keeper: AccountId,
    deposit: near_sdk::NearToken,
) -> Promise {
    ext_outlayer::ext(outlayer)
        .with_attached_deposit(deposit)
        .with_static_gas(OUTLAYER_GAS)
        .with_unused_gas_weight(1)
        .request_execution(
            source_json(source),
            Some(limits_json(limits)),
            Some(input),
            None,
            Some("Json".to_string()),
            Some(keeper),
            None,
        )
}
