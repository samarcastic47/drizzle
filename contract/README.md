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
- replay: 60 s rounds looping over the 168 historical hours from hour 497280
  (2026-09-24T00:00Z … 2026-09-30T23:00Z); live: 3600 s rounds, `replay_hours = 0`.
  Round `r` observes `first_hour + r % replay_hours`, so replay never asks
  Open-Meteo for a future (or >16-day-old) hour.

## Methods

All names, arguments, deposits, views, `RoundLog` and events are in
[`docs/interface.md`](../docs/interface.md). Summary:

| Method | Kind | Notes |
|---|---|---|
| `new(config)` | init, private | validates 12 cities, `evap_bps <= 10000`, threshold <= cap, `round_seconds > 0` |
| `claim_seed()` | payable | registers storage if needed (bond refunded above the minimum), mints 100 DRIZZLE once |
| `plant(city, amount)` | 1 yocto | `amount >= 1 DRIZZLE`; rejected when the worst-case share price would round it below `MIN_SHARES_OUT` (1e6 shares). FT transfer user -> contract, becomes pending, sprouts after `next_round` settles. Live mode freezes it once the round's hour closes |
| `uproot(city, shares?)` | 1 yocto | `null` = all shares + unsprouted pending; a partial uproot must leave 0 shares or >= 1 DRIZZLE of value; last holder sweeps the pool; same live-mode close freeze |
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
- when a pool becomes worthless (price below 1e-6, or empty), settlement resets
  it: the remaining dust is burned, old shares are retired via a city `epoch`
  bump (uprooting them returns 0 instead of panicking) and the round's pending
  sprouts 1:1. `planters` is then exactly that round's pending holders.
- `ft_transfer` / `ft_transfer_call` to the contract account are rejected
  (`use plant`), so tokens can never be stranded outside the pool accounting.
- In **live** mode, `plant`/`uproot` freeze once the round's hour has closed
  (`now >= (hour+1)*3600`), so nobody can dodge (or enter before) the evaporation
  burn after the weather is public. Replay is exempt because its hours are
  historical by design; `rain_dance` is never frozen.

## Storage

`storage_balance_bounds().min` is the bond for one FT account **plus** the 12
lazily created `UserCity` entries, measured with `env::storage_usage()` during
`new()`. On `near-sandbox` it is **31 990 000 000 000 000 000 000 yoctoNEAR
(0.03199 NEAR)** for 3 199 bytes; that covers a 64-character account id, the 12
settled bucket entries the account can pin, and the `seeded` entry that is kept
forever. `storage_unregister` refunds the bond minus the retained seed entry. Excess deposits are refunded; `storage_unregister` refunds the
bond and requires city positions to be uprooted first.

## Gas

Measured on `near-sandbox` by `tests/sandbox.rs` (mock OutLayer, so the
`request_execution` receipt is cheaper than the real one; the callback is ours):

| Scenario | `tick()` tx total | `on_weather` receipt |
|---|---|---|
| round 0: 12 pending sprout, 0 payouts | 9.428 Tgas | 4.683 Tgas |
| round 1: 12 payouts, 1 planter | 10.448 Tgas | 5.683 Tgas |
| round 2: same, 50 planters | 10.540 Tgas | 5.774 Tgas |
| 2-city round (settle test) | 7.659–7.814 Tgas | 2.916–3.057 Tgas |

`on_weather` is ~5.8 Tgas with every city paying out (well under the 15 Tgas
budget and the 60 Tgas attached to the callback), and the 1-vs-50 comparison
moves it by ~0.13 Tgas: settlement is O(cities), not O(users).

## Notes / known limits

- OutLayer never resumes a stuck worker, so `in_flight` self-expires after 600 s
  and a retry is allowed; a late callback for the old request is ignored.
- The contract cannot know the `outlayer_request_id` by itself; the oracle echoes
  `NEAR_REQUEST_ID` in its stdout, which is what rounds carry.
- Replay mode uses historical hours (a player can look them up); live mode is the
  real forecasting game. In replay, hours loop every `config.replay_hours`
  (168), so rounds can run forever.
- `RoundLog` storage is pruned to the newest `ROUND_LOG_RETENTION = 240` rounds
  inside `on_weather`; `get_round_log` returns `null` for a pruned round and
  `get_round_state.oldest_round_log` tells clients where the window starts.
- Sprout ratios live in refcounted `Bucket` entries, one per `(round, city)` that
  had pending. The bucket is created by the first `plant` of that round and
  deleted when its last holder sprouts, so the map is bounded by live positions,
  not by the number of rounds, and the registration bond pre-pays the 12 buckets
  an account can pin.
