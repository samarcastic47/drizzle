//! End-to-end sandbox tests against a real NEAR sandbox and a mock OutLayer
//! contract that implements the live `request_execution` ABI.
//!
//! Run with: `cargo test --test sandbox -- --nocapture`
//!
//! Both WASMs are built from source by `near-workspaces::compile_project` (once
//! per test process), so the tests always exercise the current code.

use near_workspaces::network::Sandbox;
use near_workspaces::result::ExecutionFinalResult;
use near_workspaces::types::{Gas, NearToken};
use near_workspaces::{Account, Contract, Worker};
use serde_json::{json, Value};
use tokio::sync::OnceCell;

const DEC: u128 = 1_000_000_000_000_000_000;
/// 0.01 NEAR, as pinned in `docs/interface.md` / the replay deploy.
const ORACLE_DEPOSIT: u128 = 10_000_000_000_000_000_000_000;
const FIRST_HOUR: u64 = 497_280;
const NUM_CITIES: usize = 12;

/// name, lat_e4, lon_e4 — the fixed order of `HANDOFF.md` §1.
const CITIES: [(&str, i32, i32); NUM_CITIES] = [
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
];

static DRIZZLE_WASM: OnceCell<Vec<u8>> = OnceCell::const_new();
static MOCK_WASM: OnceCell<Vec<u8>> = OnceCell::const_new();

async fn drizzle_wasm() -> anyhow::Result<Vec<u8>> {
    let wasm = DRIZZLE_WASM
        .get_or_try_init(|| async {
            eprintln!("[sandbox] building contract wasm ...");
            let wasm = near_workspaces::compile_project(env!("CARGO_MANIFEST_DIR")).await?;
            eprintln!("[sandbox] contract wasm: {} bytes", wasm.len());
            Ok::<_, near_workspaces::error::Error>(wasm)
        })
        .await?;
    Ok(wasm.clone())
}

async fn mock_wasm() -> anyhow::Result<Vec<u8>> {
    let wasm = MOCK_WASM
        .get_or_try_init(|| async {
            let dir = format!("{}/tests/mock_outlayer", env!("CARGO_MANIFEST_DIR"));
            eprintln!("[sandbox] building mock outlayer wasm ...");
            let wasm = near_workspaces::compile_project(&dir).await?;
            eprintln!("[sandbox] mock wasm: {} bytes", wasm.len());
            Ok::<_, near_workspaces::error::Error>(wasm)
        })
        .await?;
    Ok(wasm.clone())
}

fn config_json(oracle: &near_workspaces::AccountId, replay: bool, genesis_ts: u64) -> Value {
    json!({
        "config": {
            "name": "Drizzle",
            "symbol": "DRIZZLE",
            "decimals": 18,
            "icon": null,
            "cities": CITIES
                .iter()
                .map(|(name, lat, lon)| json!({"name": name, "lat_e4": lat, "lon_e4": lon}))
                .collect::<Vec<_>>(),
            "emission_per_round": (100 * DEC).to_string(),
            "evap_bps": 10,
            "rain_threshold_tenths": 2,
            "cap_tenths": 50,
            "seed_amount": (100 * DEC).to_string(),
            "keeper_tip": DEC.to_string(),
            "round_seconds": 60,
            "genesis_ts": genesis_ts,
            "first_hour": FIRST_HOUR,
            "replay": replay,
            "oracle": {
                "outlayer": oracle.to_string(),
                "source": {
                    "WasmUrl": {
                        "url": "https://test.fastfs.io/drizzle-dev.testnet/outlayer.testnet/7d708ee751d708e8bfd331785204190e1741ed1ca0ce5f76dcf4317e75a7128b.wasm",
                        "hash": "7d708ee751d708e8bfd331785204190e1741ed1ca0ce5f76dcf4317e75a7128b",
                        "build_target": "wasm32-wasip2",
                    }
                },
                "resource_limits": {
                    "max_instructions": 5_000_000_000u64,
                    "max_memory_mb": 64u32,
                    "max_execution_seconds": 20u64,
                },
                "deposit": ORACLE_DEPOSIT.to_string(),
            }
        }
    })
}

struct Env {
    worker: Worker<Sandbox>,
    drizzle: Contract,
    mock: Contract,
}

async fn deploy_all(replay: bool, genesis_ts: u64) -> anyhow::Result<Env> {
    let worker = near_workspaces::sandbox().await?;

    let mock = worker.dev_deploy(&mock_wasm().await?).await?;
    mock.call("new")
        .gas(Gas::from_tgas(50))
        .transact()
        .await?
        .into_result()?;

    let drizzle = worker.dev_deploy(&drizzle_wasm().await?).await?;
    // `new` is #[private]; dev_deploy gives us the account's own key, so the
    // predecessor is the contract account itself.
    drizzle
        .call("new")
        .args_json(config_json(mock.id(), replay, genesis_ts))
        .gas(Gas::from_tgas(150))
        .transact()
        .await?
        .into_result()?;

    Ok(Env {
        worker,
        drizzle,
        mock,
    })
}

impl Env {
    async fn set_response(&self, response: Value) -> anyhow::Result<()> {
        self.mock
            .call("set_response")
            .args_json(json!({ "response": response }))
            .gas(Gas::from_tgas(30))
            .transact()
            .await?
            .into_result()?;
        Ok(())
    }

    async fn set_empty(&self, empty: bool) -> anyhow::Result<()> {
        self.mock
            .call("set_empty")
            .args_json(json!({ "empty": empty }))
            .gas(Gas::from_tgas(30))
            .transact()
            .await?
            .into_result()?;
        Ok(())
    }

    async fn storage_min(&self) -> anyhow::Result<u128> {
        let bounds: Value = self
            .drizzle
            .view("storage_balance_bounds")
            .await?
            .json()?;
        Ok(bounds["min"].as_str().unwrap().parse()?)
    }

    async fn seed(&self, account: &Account) -> anyhow::Result<()> {
        let min = self.storage_min().await?;
        account
            .call(self.drizzle.id(), "claim_seed")
            .deposit(NearToken::from_yoctonear(min))
            .gas(Gas::from_tgas(50))
            .transact()
            .await?
            .into_result()?;
        Ok(())
    }

    async fn plant(&self, account: &Account, city: u8, amount: u128) -> anyhow::Result<()> {
        account
            .call(self.drizzle.id(), "plant")
            .args_json(json!({ "city": city, "amount": amount.to_string() }))
            .deposit(NearToken::from_yoctonear(1))
            .gas(Gas::from_tgas(40))
            .transact()
            .await?
            .into_result()?;
        Ok(())
    }

    async fn uproot(
        &self,
        account: &Account,
        city: u8,
        shares: Option<u128>,
    ) -> anyhow::Result<u128> {
        let result = account
            .call(self.drizzle.id(), "uproot")
            .args_json(json!({
                "city": city,
                "shares": shares.map(|s| s.to_string()),
            }))
            .deposit(NearToken::from_yoctonear(1))
            .gas(Gas::from_tgas(60))
            .transact()
            .await?;
        let value: String = result.json()?;
        Ok(value.parse()?)
    }

    async fn tick(&self, keeper: &Account) -> anyhow::Result<ExecutionFinalResult> {
        Ok(keeper
            .call(self.drizzle.id(), "tick")
            .deposit(NearToken::from_yoctonear(ORACLE_DEPOSIT))
            .gas(Gas::from_tgas(300))
            .transact()
            .await?)
    }

    async fn round_state(&self) -> anyhow::Result<Value> {
        Ok(self.drizzle.view("get_round_state").await?.json()?)
    }

    async fn stats(&self) -> anyhow::Result<Value> {
        Ok(self.drizzle.view("get_stats").await?.json()?)
    }

    async fn balance_of(&self, account: &Account) -> anyhow::Result<u128> {
        let value: String = self
            .drizzle
            .view("ft_balance_of")
            .args_json(json!({ "account_id": account.id() }))
            .await?
            .json()?;
        Ok(value.parse()?)
    }
}

/// A rain payload for `round`, matching the live oracle stdout shape.
fn weather(round: u64, mm: [u16; NUM_CITIES]) -> Value {
    json!({
        "hour": FIRST_HOUR + round,
        "mm_tenths": mm,
        "source": "open-meteo",
        "outlayer_request_id": format!("req-{}", 22_000 + round),
    })
}

fn zero_mm() -> [u16; NUM_CITIES] {
    [0u16; NUM_CITIES]
}

/// The panic message / error text of a failed execution (plus its logs).
fn failure_text(result: &ExecutionFinalResult) -> String {
    let err = result
        .clone()
        .into_result()
        .err()
        .map(|failure| format!("{failure:?}"))
        .unwrap_or_default();
    format!("{err} {:?}", result.logs())
}

/// Prints per-receipt gas and returns (tick tx total, on_weather receipt) in Tgas.
fn report_gas(
    label: &str,
    result: &ExecutionFinalResult,
    contract_id: &near_workspaces::AccountId,
) -> (f64, f64) {
    let total = result.total_gas_burnt.as_gas() as f64 / 1e12;
    let mut callback = 0.0;
    for outcome in result.receipt_outcomes() {
        let gas = outcome.gas_burnt.as_gas() as f64 / 1e12;
        eprintln!(
            "[{label}] receipt on {}: {gas:.3} Tgas ({} log lines)",
            outcome.executor_id,
            outcome.logs.len()
        );
        if &outcome.executor_id == contract_id
            && outcome.logs.iter().any(|line| {
                line.contains("\"event\":\"round\"") && line.contains("\"standard\":\"drizzle\"")
            })
        {
            callback = gas;
        }
    }
    eprintln!("[{label}] tick tx TOTAL {total:.3} Tgas; on_weather receipt {callback:.3} Tgas");
    assert!(callback > 0.0, "{label}: on_weather receipt not found");
    assert!(
        callback < 60.0,
        "{label}: on_weather used {callback:.3} Tgas > the 60 Tgas attached"
    );
    (total, callback)
}

// ---------------------------------------------------------------- tests

#[tokio::test]
async fn success_settles_cities_and_pays_planters() -> anyhow::Result<()> {
    let env = deploy_all(true, 0).await?;
    let alice = env.worker.dev_create_account().await?;

    let min = env.storage_min().await?;
    eprintln!(
        "[storage] storage_balance_bounds.min = {min} yoctoNEAR ({:.6} NEAR)",
        min as f64 / 1e24
    );
    let supply0: String = env
        .drizzle
        .view("ft_total_supply")
        .await?
        .json()?;
    assert_eq!(supply0, "0");

    env.seed(&alice).await?;
    env.plant(&alice, 0, 50 * DEC).await?;
    env.plant(&alice, 4, 20 * DEC).await?;

    let mut mm = zero_mm();
    mm[0] = 25;
    env.set_response(weather(0, mm)).await?;
    let first_tick = env.tick(&alice).await?;
    assert!(first_tick.is_success(), "tick failed: {:?}", first_tick.logs());
    report_gas("round0", &first_tick, env.drizzle.id());

    let state = env.round_state().await?;
    assert_eq!(state["next_round"], 1);
    assert!(state["in_flight"].is_null());
    assert_eq!(state["next_hour"], FIRST_HOUR + 1);

    let log: Value = env
        .drizzle
        .view("get_round_log")
        .args_json(json!({ "round": 0 }))
        .await?
        .json()?;
    assert_eq!(log["hour"], FIRST_HOUR);
    assert_eq!(log["mm_tenths"][0], 25);
    assert_eq!(log["minted"][0], "0", "no shares at round 0 -> no mint");
    assert_eq!(log["keeper"], alice.id().as_str());
    assert_eq!(log["outlayer_request_id"], "req-22000");
    assert!(log["tick_block_height"].as_u64().unwrap() > 0);

    let cities: Value = env.drizzle.view("get_cities").await?.json()?;
    assert_eq!(cities.as_array().unwrap().len(), NUM_CITIES);
    assert_eq!(cities[0]["value"], (50 * DEC).to_string());
    assert_eq!(cities[0]["shares"], (50 * DEC).to_string());
    assert_eq!(cities[0]["planters"], 1);
    assert_eq!(cities[0]["last_rain_round"], 0);

    // The mock saw the exact input the oracle expects.
    let input: String = env
        .mock
        .view("get_last_input")
        .await?
        .json::<Option<String>>()?
        .expect("mock recorded input");
    let input: Value = serde_json::from_str(&input)?;
    assert_eq!(input["hour"], FIRST_HOUR);
    assert_eq!(input["cities"].as_array().unwrap().len(), NUM_CITIES);
    assert_eq!(input["cities"][0][0], 603_900);
    assert_eq!(input["cities"][11][1], 552_700);

    // A rainy round pays the planter.
    mm[0] = 50;
    env.set_response(weather(1, mm)).await?;
    let second = env.tick(&alice).await?;
    assert!(second.is_success(), "tick failed: {:?}", second.logs());
    report_gas("round1", &second, env.drizzle.id());

    let cities: Value = env.drizzle.view("get_cities").await?.json()?;
    assert_eq!(cities[0]["value"], (150 * DEC).to_string());
    // 100 seed + 100 round-1 rain + 2 keeper tips - 0.002 evaporated from Tokyo.
    assert_eq!(
        env.stats().await?["total_supply"],
        (202 * DEC - 20_000_000_000_000_000u128).to_string(),
        "seed + rain + keeper tips - evaporation"
    );

    let before = env.balance_of(&alice).await?;
    let got = env.uproot(&alice, 0, None).await?;
    assert_eq!(got, 150 * DEC);
    assert_eq!(env.balance_of(&alice).await?, before + got);

    let cities: Value = env.drizzle.view("get_cities").await?.json()?;
    assert_eq!(cities[0]["value"], "0");
    assert_eq!(cities[0]["planters"], 0);

    // Events are NEP-297 shaped.
    let all_logs: Vec<String> = second.logs().iter().map(|s| s.to_string()).collect();
    assert!(all_logs
        .iter()
        .any(|l| l.contains("EVENT_JSON:") && l.contains("\"event\":\"round\"")));
    Ok(())
}

#[tokio::test]
async fn failed_payloads_do_not_advance_and_can_retry() -> anyhow::Result<()> {
    let env = deploy_all(true, 0).await?;
    let alice = env.worker.dev_create_account().await?;
    env.seed(&alice).await?;
    env.plant(&alice, 0, 10 * DEC).await?;

    let failures: Vec<(&str, Value)> = vec![
        ("null", Value::Null),
        ("malformed", json!("not-an-object")),
        (
            "wrong hour",
            json!({"hour": FIRST_HOUR + 9, "mm_tenths": zero_mm(), "source": "open-meteo"}),
        ),
        (
            "short mm_tenths",
            json!({"hour": FIRST_HOUR, "mm_tenths": [0u16, 0, 0], "source": "open-meteo"}),
        ),
    ];

    for (name, response) in failures {
        env.set_response(response).await?;
        let result = env.tick(&alice).await?;
        // The callback handles bad data gracefully (no panic), so the tx succeeds;
        // it must still emit drizzle_round_failed and leave the round unsettled.
        assert!(result.is_success(), "{name}: tick failed: {:?}", result.logs());
        assert!(
            result.logs().iter().any(|l| l.contains("\"event\":\"round_failed\"")),
            "{name}: no drizzle_round_failed event: {:?}",
            result.logs()
        );
        let state = env.round_state().await?;
        assert_eq!(state["next_round"], 0, "{name}: round advanced");
        assert!(state["in_flight"].is_null(), "{name}: in_flight not cleared");
        assert_eq!(env.stats().await?["rounds_settled"], 0);

        // The keeper can retry immediately (same round, in_flight cleared).
        env.set_response(weather(0, zero_mm())).await?;
    }

    let ok = env.tick(&alice).await?;
    assert!(ok.is_success(), "retry failed: {:?}", ok.logs());
    assert_eq!(env.round_state().await?["next_round"], 1);
    let log: Value = env
        .drizzle
        .view("get_round_log")
        .args_json(json!({ "round": 0 }))
        .await?
        .json()?;
    assert_eq!(log["hour"], FIRST_HOUR);
    Ok(())
}

#[tokio::test]
async fn private_callback_early_tick_and_single_flight() -> anyhow::Result<()> {
    let env = deploy_all(true, 0).await?;
    let alice = env.worker.dev_create_account().await?;

    // Direct `on_weather` call is rejected: only the contract itself may call it.
    let direct = alice
        .call(env.drizzle.id(), "on_weather")
        .args_json(json!({ "round": 0, "keeper": alice.id() }))
        .gas(Gas::from_tgas(40))
        .transact()
        .await?;
    assert!(direct.is_failure(), "on_weather must not be callable directly");
    assert!(
        format!("{:?}", direct.into_result().unwrap_err()).contains("private"),
        "expected a private-method panic"
    );

    // Ticks are rejected before the round is due (replay genesis in the future).
    let early_env = deploy_all(true, 4_000_000_000).await?;
    let bob = early_env.worker.dev_create_account().await?;
    let early = early_env.tick(&bob).await?;
    assert!(early.is_failure(), "a not-yet-due round must not be ticked");
    assert!(failure_text(&early).contains("round not due"));
    assert!(early_env.round_state().await?["in_flight"].is_null());
    assert_eq!(early_env.round_state().await?["next_round"], 0);

    // One tick at a time. The empty promise result leaves the request in flight
    // (as a stuck OutLayer worker would), so the next tick is rejected.
    env.set_empty(true).await?;
    let first = env.tick(&alice).await?;
    assert!(first.is_failure(), "empty callback result must fail: {:?}", first.logs());
    assert_eq!(env.round_state().await?["in_flight"]["round"], 0);

    let second = env.tick(&alice).await?;
    assert!(
        second.is_failure(),
        "a second tick while one is in flight must fail"
    );
    assert!(
        failure_text(&second).contains("tick in flight"),
        "{}",
        failure_text(&second)
    );
    assert_eq!(env.round_state().await?["next_round"], 0);
    env.set_empty(false).await?;
    Ok(())
}

#[tokio::test]
async fn in_flight_expires_after_600s_and_allows_a_retry() -> anyhow::Result<()> {
    let env = deploy_all(true, 0).await?;
    let alice = env.worker.dev_create_account().await?;
    env.seed(&alice).await?;
    env.plant(&alice, 0, 10 * DEC).await?;

    // An empty promise result simulates an OutLayer worker that never resumes:
    // the callback cannot deserialize it, so in_flight stays set.
    env.set_empty(true).await?;
    let stuck = env.tick(&alice).await?;
    assert!(stuck.is_failure(), "the callback must fail on empty bytes");

    let state = env.round_state().await?;
    assert_eq!(state["next_round"], 0);
    assert_eq!(state["in_flight"]["round"], 0);

    // Immediately retrying is blocked.
    let blocked = env.tick(&alice).await?;
    assert!(blocked.is_failure(), "tick in flight must be rejected");

    // Time passes: the lock expires even though no callback ever arrived.
    env.worker.fast_forward(2_000).await?;
    env.set_empty(false).await?;
    env.set_response(weather(0, zero_mm())).await?;
    let retried = env.tick(&alice).await?;
    assert!(
        retried.is_success(),
        "retry failed: {} {:?}",
        failure_text(&retried),
        retried.logs()
    );

    let state = env.round_state().await?;
    assert_eq!(state["next_round"], 1);
    assert!(state["in_flight"].is_null());
    assert_eq!(env.stats().await?["rounds_settled"], 1);
    Ok(())
}

#[tokio::test]
async fn callback_gas_is_independent_of_user_count() -> anyhow::Result<()> {
    let env = deploy_all(true, 0).await?;
    let alice = env.worker.dev_create_account().await?;
    env.seed(&alice).await?;

    // Give alice shares in every city so each settlement does 12 pool payouts.
    for city in 0..NUM_CITIES as u8 {
        env.plant(&alice, city, 8 * DEC).await?;
    }
    let all_rain: [u16; NUM_CITIES] = [50; NUM_CITIES];

    // Round 0: 12 pending sprout (1 position), no shares yet.
    env.set_response(weather(0, all_rain)).await?;
    let tick0 = env.tick(&alice).await?;
    assert!(tick0.is_success(), "{:?}", tick0.logs());
    report_gas("gas/12-sprouts/1-user", &tick0, env.drizzle.id());

    // Round 1: 12 payouts + 1 sprout, one planter.
    env.plant(&alice, 0, DEC).await?;
    env.set_response(weather(1, all_rain)).await?;
    let tick1 = env.tick(&alice).await?;
    assert!(tick1.is_success(), "{:?}", tick1.logs());
    let (total_one, gas_one) = report_gas("gas/1-user", &tick1, env.drizzle.id());

    // Round 2: identical weather and identical single-city pending, but 50 planters.
    let mut planters = 0u32;
    for _ in 0..50 {
        let user = env.worker.dev_create_account().await?;
        env.seed(&user).await?;
        env.plant(&user, 0, DEC).await?;
        planters += 1;
    }
    env.set_response(weather(2, all_rain)).await?;
    let tick2 = env.tick(&alice).await?;
    assert!(tick2.is_success(), "{:?}", tick2.logs());
    let (total_fifty, gas_fifty) = report_gas("gas/50-users", &tick2, env.drizzle.id());

    eprintln!(
        "[gas] on_weather: 1 user {gas_one:.3} Tgas (tx {total_one:.3}) vs 50 users {gas_fifty:.3} Tgas (tx {total_fifty:.3})"
    );
    assert!(
        (gas_fifty - gas_one).abs() < 0.5,
        "callback gas must not depend on the user count: {gas_one:.3} vs {gas_fifty:.3} Tgas"
    );

    // The 50 rows exist in state but the settlement did not touch them.
    let cities: Value = env.drizzle.view("get_cities").await?.json()?;
    assert_eq!(cities[0]["planters"], 51);
    assert_eq!(planters, 50);
    Ok(())
}

