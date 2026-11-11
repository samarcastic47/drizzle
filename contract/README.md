# Drizzle contract ($DRIZZLE)

A NEP-141 memecoin on NEAR testnet that mints only when it rains in 12 real
cities. Every round anyone calls `tick()`; the contract asks **OutLayer**
(`outlayer.testnet`, TEE-attested) to run our pinned weather-oracle WASM
(Open-Meteo), and the `on_weather` callback settles the city pools.

The authoritative method/argument/view/event reference is
[`docs/interface.md`](../docs/interface.md). The pool math is
[`src/pool.rs`](src/pool.rs) (pure integer, no SDK).

## Layout

| File | Role |
|---|---|
| `src/pool.rs` | pure pool math (settle, sprout, uproot, evaporation) — verified against `tools/ref_settle.py` |
| `src/config.rs` | immutable `Config` / `OracleConfig` (borsh + JSON) |
| `src/oracle.rs` | `request_execution` cross-contract call, oracle input builder, defensive payload parsing |
| `src/user.rs` | persisted per-city pool and per-user position |
| `src/events.rs` | NEP-297 `drizzle` events |
| `src/views.rs` | view/round-log structs |
| `src/lib.rs` | the contract: FT, storage, plant/uproot/rain_dance, tick/on_weather, views |

## Build

Rust **1.85.0** (near-sdk 5.24.0; newer SDKs need 1.95). Do not run `rustup`.

```bash
cd contract
cargo near build non-reproducible-wasm   # -> target/near/drizzle.wasm
```

## Test

```bash
cd contract
cargo test --lib                       # unit tests: invariants 1-6, tick/callback rules
cargo test --test ref_cross_check      # pool math vs the Python reference over 48 h
cargo test --test sandbox -- --nocapture   # near-sandbox + mock OutLayer, prints gas
```

The sandbox suite builds both WASMs from source (`near-workspaces::compile_project`)
— the contract and `tests/mock_outlayer/`, a tiny crate implementing the live
seven-argument `request_execution` ABI — so it always exercises the current code.
`contract/.cargo/config.toml` points `NEAR_SANDBOX_BIN_PATH` at the cached
`contract/target/near-sandbox` binary (near-sandbox 2.6.2) because the network is
too slow to download it; on a clean clone, delete that file and near-workspaces
fetches the sandbox once. It covers: successful settlement and
views/events, `null`/malformed/wrong-hour payloads, direct `on_weather` calls,
early ticks, the single-flight lock, its 600 s expiry, and a gas comparison with
1 vs 50 planters.

## Deploy

```bash
cd contract
python3 scripts/init_args.py --replay --genesis-ts "$(date +%s)"   # prints {"config": {...}}
./scripts/deploy.sh drizzle-replay.testnet replay
./scripts/deploy.sh drizzle-live.testnet live
```

`new(config)` is `#[private]`: the transaction must be signed by the contract
account itself (the deploy script uses the account's key). There is no admin and
no setter — every parameter is fixed at init.

Live values baked into `scripts/init_args.py`:

- outlayer: `outlayer.testnet`
- oracle: `https://test.fastfs.io/drizzle-dev.testnet/outlayer.testnet/7d708ee751d708e8bfd331785204190e1741ed1ca0ce5f76dcf4317e75a7128b.wasm`,
  sha256 `7d708ee751d708e8bfd331785204190e1741ed1ca0ce5f76dcf4317e75a7128b`,
  build target `wasm32-wasip2`
- resource limits: 5 000 000 000 instructions / 64 MB / 20 s; deposit 0.01 NEAR
- emission 100 DRIZZLE/round, evaporation 10 bps, rain threshold 0.2 mm, storm cap 5.0 mm,
  seed 100 DRIZZLE, keeper tip 1 DRIZZLE
- replay: 60 s rounds from hour 497280 (2026-09-24T00:00Z); live: 3600 s rounds

## Methods

All names, arguments, deposits, views, `RoundLog` and events are in
[`docs/interface.md`](../docs/interface.md). Summary:

| Method | Kind | Notes |
|---|---|---|
| `new(config)` | init, private | validates 12 cities, `evap_bps <= 10000`, threshold <= cap, `round_seconds > 0` |
| `claim_seed()` | payable | registers storage if needed (bond refunded above the minimum), mints 100 DRIZZLE once |
| `plant(city, amount)` | 1 yocto | FT transfer user -> contract, becomes pending, sprouts after `next_round` settles |
| `uproot(city, shares?)` | 1 yocto | `null` = all shares + unsprouted pending; last holder sweeps the pool |
| `rain_dance(city, amount)` | 1 yocto | burns the caller's tokens into the city jackpot |
| `tick()` | payable (`oracle.deposit`) | one request at a time; 600 s self-expiry; `.then(on_weather)` |
| `on_weather(round, keeper)` | private callback | settles 12 pools, mints/burns, stores `RoundLog` |
| NEP-141 / NEP-145 | standard | `ft_transfer`, `ft_transfer_call`, `ft_resolve_transfer`, `ft_balance_of`, `ft_total_supply`, `ft_metadata`, `storage_*` |
| views | – | `get_config`, `get_round_state`, `get_cities`, `get_user`, `get_round_log(s)`, `get_stats` |

### Money flow (why the invariants are exact)

`total_supply == Σ ft_balance_of(all accounts)`, and the contract's own FT
balance is exactly `Σ pool.value + Σ pool.pending_total`:

- `plant`: transfer user -> contract; `uproot`: transfer contract -> user.
- rain: mint to the contract; evaporation: burn from the contract.
- `rain_dance`: burn from the user; seed and keeper tip: mint to user/keeper.
- sprout dust (floor rounding in shares) stays in `pool.value`, which is why the
  last holder receives the whole remaining pool value when `planters` hits 0.

## Storage

`storage_balance_bounds().min` is the bond for one FT account **plus** the 12
lazily created `UserCity` entries, measured with `env::storage_usage()` during
`new()`. On `near-sandbox` it is **19 380 000 000 000 000 000 000 yoctoNEAR
(0.01938 NEAR)** for 1 938 bytes; that covers a 64-character account id, which is
the worst case. Excess deposits are refunded; `storage_unregister` refunds the
bond and requires city positions to be uprooted first.

## Gas

Measured on `near-sandbox` by `tests/sandbox.rs` (mock OutLayer, so the
`request_execution` receipt is cheaper than the real one; the callback is ours):

| Scenario | `tick()` tx total | `on_weather` receipt |
|---|---|---|
| round 0: 12 pending sprout, 0 payouts | 8.320 Tgas | 3.588 Tgas |
| round 1: 12 payouts, 1 planter | 10.307 Tgas | 5.553 Tgas |
| round 2: same, 50 planters | 10.414 Tgas | 5.661 Tgas |
| 2-city round (settle test) | 7.417–7.770 Tgas | 2.686–3.026 Tgas |

`on_weather` is ~5.7 Tgas with every city paying out, far below the 60 Tgas
attached to the callback, and the 1-vs-50 comparison moves it by ~0.1 Tgas:
settlement is O(cities), not O(users).

## Notes / known limits

- OutLayer never resumes a stuck worker, so `in_flight` self-expires after 600 s
  and a retry is allowed; a late callback for the old request is ignored.
- The contract cannot know the `outlayer_request_id` by itself; the oracle echoes
  `NEAR_REQUEST_ID` in its stdout, which is what rounds carry.
- Replay mode uses historical hours (a player can look them up); live mode is the
  real forecasting game.
