# STATUS — Drizzle ($DRIZZLE)

_Updated: 2026-11-11 (session start)_

## Now
| WP | Agent | State |
|---|---|---|
| WP-0 Setup | orchestrator + human | Installed: pnpm 12.18, near-cli-rs 0.31.1 (~/.local/bin), wasmtime 50.0.1 (~/.wasmtime/bin), outlayer 0.2.3 (~/.local/bin), cargo-near 0.23.0. Rust 1.85 is being repaired: parallel cargo runs raced rustup auto-install. Rule: `RUSTUP_AUTO_INSTALL=0`, no rustup from workers. Network ~30 KB/s. Human: accounts/logins pending |
| WP-2 Oracle | `task` WP2Oracle | Code done; blocked on toolchain |
| WP-4 Pool math | `task` WP4PoolMath | Code done; blocked on toolchain |
| WP-8 Frontend | `task` WP8Frontend | Running (mock data first) |

## Done
- WP-1 Doc scout: `docs/notes/outlayer.md`. It confirms the live 7-arg `request_execution` ABI from the chain schema, the raw-stdout callback (no wrapper), the `NEAR_REQUEST_ID` env var for the guest, `WasmUrl` hash checks inside the enclave, the ≈0.0045 NEAR estimate for a 30 s job, and the attestation URL `https://app.outlayer.ai/attestation/{id}` (id mapping unverified).
- `docs/supply.svg` (part of WP-10): `python3 tools/supply_chart.py` → `evap 0.10% (chosen), mixed players  final supply 110,314` (matches IDEA.md 110.3k). `sim.run` now also returns `curve_daily`.
- `docs/interface.md`: the shared contract interface (methods, views, events) for contract/keeper/web.
- WP-0 (human part): `drizzle-dev.testnet` imported into near-cli-rs (keychain signing verified with a signed, unsent tx; 10 NEAR). `outlayer whoami` → drizzle-dev.testnet on testnet.

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

## Blockers / waiting on human
- Human creates top-level `drizzle-replay.testnet` (free) and imports it into near-cli-rs; needed by WP-6.
- `drizzle.testnet` is TAKEN by a third party (4 unrelated keys, 200 NEAR). Proposed live name: `drizzle-live.testnet` (free); needs the human's OK + creation before WP-11.

## Costs
- Per tick: TBD (WP-3)
- Faucet balance: TBD
