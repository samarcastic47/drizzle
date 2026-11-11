# STATUS — Drizzle ($DRIZZLE)

_Updated: 2026-11-11 (session start)_

## Now
| WP | Agent | State |
|---|---|---|
| WP-0 Setup | done | Toolchain complete |
| WP-6 Deploy replay | orchestrator | FINAL contract (sha256 fe779482…1001) deployed fresh to `drizzle-replay.testnet` (account reset via faucet) and `drizzle-live.testnet` (live, first_hour 498446). Keepers run as persistent services `drizzle-keeper` / `drizzle-keeper-live` on fastnear RPC; web served at http://localhost:4321 (`drizzle-web`). Human manual test pending (TESTING.md) |
| WP-9 Review | done | Re-review: all F/S items closed; final batch R1 (dead shares 1e12, guard → ≥1 share), R2 (live exit after close+24 h), R3 (config bounds), R4 (re-registration discount). 46 unit tests |
| Next | – | After the human's test: delete keys (human OK), hosting (human), video/X/submission |

## Done
- WP-1 Doc scout: `docs/notes/outlayer.md`. It confirms the live 7-arg `request_execution` ABI from the chain schema, the raw-stdout callback (no wrapper), the `NEAR_REQUEST_ID` env var for the guest, `WasmUrl` hash checks inside the enclave, the ≈0.0045 NEAR estimate for a 30 s job, and the attestation URL `https://app.outlayer.ai/attestation/{id}` (id mapping unverified).
- `docs/supply.svg` (part of WP-10): `python3 tools/supply_chart.py` → `evap 0.10% (chosen), mixed players  final supply 110,314` (matches IDEA.md 110.3k). `sim.run` now also returns `curve_daily`.
- `docs/interface.md`: the shared contract interface (methods, views, events) for contract/keeper/web.
- WP-0 (human part): `drizzle-dev.testnet` imported into near-cli-rs (keychain signing verified with a signed, unsent tx; 10 NEAR). `outlayer whoami` → drizzle-dev.testnet on testnet.
- WP-4 Pool math: `cd contract && cargo test` → `12 passed` + `matches_python_reference_over_48_hours ... ok`; `python3 tools/ref_settle.py rain_60d.json` regenerates an identical fixture. The harness uses 100-DRIZZLE units for amounts (scale only). Commit 5466f25.
- WP-2 Oracle: `cd oracle && cargo test` → 13 passed; `./build.sh` → 467K wasm; `./verify.sh` → `MATCH` ([0,0,3,0,…] for 2026-09-24T14:00); error case (hour >92 d back) → `{"error":"open-meteo HTTP 400: …"}`, exit 1.
- WP-3 Oracle live ⛔ gate PASSED: `outlayer upload` → sha256 `7d708ee7…128b`. A manual `request_execution` (tx `7T2fBww6cqcMJQw7RNoevLT7oVCZPmJSVKLPtjYyFaoY`, request_id 22023) returned `{"hour":497294,"mm_tenths":[0,0,3,0,0,0,0,0,0,0,0,0],"outlayer_request_id":"22023","source":"open-meteo"}`. Attestation: https://app.outlayer.ai/attestation/57705?network=testnet shows the executed binary = pinned hash. Details in docs/notes/outlayer.md §11.
- WP-5 Contract: `cargo test --lib` → 29 passed (incl. the orchestrator's re-seed regression test); `ref_cross_check` → ok; sandbox → 5 passed; `cargo near build` → 366.6 KB. Gas: tick tx 10.4 Tgas, on_weather 5.66 Tgas with 50 planters vs 5.55 with 1 (O(cities)). Storage bond 0.01938 NEAR. Orchestrator fix: `storage_unregister` no longer clears `seeded` (it allowed a sybil re-seed) and now emits `ft_burn` + counts the burn for forced balances.
- WP-7 Keeper: `pnpm test` → 14/14; missing creds → exit 1. near-api-js 7.5.2.
- WP-8 Frontend: `pnpm build` and `NEXT_PUBLIC_MOCK=1 pnpm build` pass; verified in a browser at 390/1280 px (map, countdown, feed, Verify 12/12 match against live Open-Meteo, About + supply chart).
- WP-10 draft: README.md (181 lines; placeholders {{DEMO_URL}} {{REPLAY_CONTRACT}} {{LIVE_CONTRACT}} {{REPO_URL}}), MIT LICENSE.

## Decisions
- `pool.rs` uses `uint` U256 `mul_div` for all a·b/c products — 18-decimal u128 products overflow u128.
- All rounding is floor; emission dust (E·w/W remainders) is never minted; sprout dust shares stay unowned in the pool (value counted in invariant 1).
- `last_rain_round` is set whenever a city's weight > 0, even with zero shares (frontend "last rain" is about weather, not payouts).
- Sprout when `shares > 0 && value == 0`: old shares are reset to 0, then 1:1.
- Jackpot is outside circulating supply (dance burns); payout on rain is a mint.
- WP-1 is run by `scout` (read-only); orchestrator saves its report to `docs/notes/outlayer.md`.
- Last-holder sweep (WP-5): when an uproot leaves a city with `planters == 0` (no user shares or pending), the uprooter receives the entire `pool.value`, and `pool.shares` is reset to 0. Without this, unowned sprout-dust shares keep `shares > 0`, and rain would mint into an ownerless pool.
- Oracle source pinned as `{"WasmUrl":{url,hash,build_target:"wasm32-wasip2"}}` (exact sha256 of the bytes). There is no `WasmHash` variant on the live contract, and `Project` would let the owner swap the active version.
- `request_execution` uses the live 7-arg ABI (`source, resource_limits, input_data, secrets_ref, response_format:"Json", payer_account_id:keeper, params:null`). The callback type is `Result<Option<Value>, PromiseError>`, with no `OutLayerResponse` wrapper (docs/notes/outlayer.md §1–2).
- OutLayer never resumes on a stuck worker, so the `in_flight` lock self-expires after 10 min (matches §2.2).
- Open-Meteo `/v1/forecast` covers ~92 days back. The replay window (from 2026-09-24) stays valid until ~2026-12-25, so the archive fallback is cut.
- near-sdk / near-contract-standards **5.24.0** (the newest that builds on rustc 1.85; 5.30 needs 1.95). Contract Cargo.toml sets `rust-version = "1.85"`, and the lockfile is generated with `CARGO_RESOLVER_INCOMPATIBLE_RUST_VERSIONS=fallback`. near-workspaces 0.20.1.
- rustup auto-install is disabled globally (`rustup set auto-install disable`). Concurrent auto-installs corrupted the toolchain twice.
- Sandbox tests: near-sandbox-utils pinned to 0.15.0 (sandbox 2.6.2); 0.15.1 pulls sandbox 2.9.0, which near-workspaces 0.20.1 can't parse. Known-good lock: docs/notes/near-sdk-1.85.Cargo.lock.
- Verify drawer attestation link: request_id → job_id via `testnet-api.outlayer.ai/public/jobs?user_account_id=<keeper>`, then `app.outlayer.ai/attestation/<job_id>?network=testnet` (§11). That API sends no CORS header for third-party origins, so the static site falls back to the explorer link plus the request id and the request tx. The README Verify step names the job id explicitly for the demo round. No proxy: the frontend has no backend by decision.
- Known limit (README): in replay mode the hours are historical, so a player can look up upcoming replay hours on Open-Meteo. Replay demonstrates the mechanic and the oracle path; the live instance is the real forecasting game.

## Blockers / waiting on human
- Approved by the human: delete + recreate `drizzle-replay.testnet` via the faucet service once the review fixes land (clean state for the new layout).
- Approved live name: `drizzle-live.testnet` (`drizzle.testnet` belongs to a third party). Create it at WP-11.
- Human: add ~15 NEAR (faucet) to drizzle-dev before Nov 16.
- Human decisions (pre-nap, 2026-11-11): hosting deferred; keeper runs until stopped; create + deploy `drizzle-live.testnet` via the faucet service; fix + redeploy if the re-review finds bugs; seed demo activity from drizzle-dev; the human tests with a separate testnet wallet. Keys are NOT deleted yet.

## Costs
- Measured per contract tick (keeper side, OutLayer charge + tick + callback gas): **0.002396 NEAR** (round 3, live testnet).
- Human decision: 60 s replay rounds through Nov 18 (~22 NEAR). The human adds ~15 NEAR from the faucet to drizzle-dev before judging.
- Contract storage: ~0.009 NEAR per RoundLog. Retention capped at 240 logs (WP-5 fix in progress).
- drizzle-dev.testnet balance: 9.96 NEAR. drizzle-replay.testnet: 5.016 NEAR (to be returned on delete/recreate).
