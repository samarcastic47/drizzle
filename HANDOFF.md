# HANDOFF: Build "Drizzle" ($DRIZZLE) for NEAR HACKATHON

You are the **orchestrator** (Claude Opus 5.5) in an Oh-My-Pi session. Read this whole file before acting. It is self-contained: you have no other context from earlier conversations.

**Your job:** make the decisions, write precise task briefs, delegate implementation to **DeepSeek v4.1 Flash worker subagents** (`task`, `scout`, `sonic`, `reviewer`, `security-reviewer`), review their output, run the verification yourself, and keep `STATUS.md` current. Write code yourself only for small glue fixes (< ~30 lines) or when a worker has failed the same brief twice.

**Hard deadline:** development ends **2026-11-14 23:59 UTC**. Today is 2026-11-11. Plan: **Nov 12 = Day 1, Nov 13 = Day 2, Nov 14 = buffer + submission.** Judging runs Nov 16–18, and the hosted demo must stay up until at least Nov 18.

Working directory: `/home/smitjogani/Work/drizzle/`. Already present:
- `IDEA.md`: product spec and pitch. Read §2, §3, §5 and §6.
- `sim.py`, `rain_60d.json`, `sim_results.txt`: supply simulation on real Open-Meteo data (12 cities × 1,440 hours). These justify the parameters below.

---

## 0. The human and the rules (read first)

The human (team lead) does anything involving identity, money, publication or irreversibility. **Stop and ask them** before:
- creating NEAR accounts, logging in (`outlayer login`, `near` credentials), or funding from a faucet
- **deleting the contract's access keys** (irreversible; do it only after the reviewer + security-reviewer pass)
- making the GitHub repo public, pushing, or force-pushing
- deploying the frontend to a public host
- anything on **mainnet** (out of scope; never)
- posting on X, or sending the submission (`POST https://nearhackathon.com/api/applications`). Print the full JSON payload and wait for an explicit yes. There is one attempt per repo.

**Pre-approved after the human says "go" at session start:** testnet deploys of `drizzle*.testnet` contracts, testnet `near call`s, and OutLayer testnet executions paid from the dev account. Confirm this with the human in your first message.

Never commit secrets: keys go in env vars, and `.env` goes in `.gitignore`. Live rules: https://nearhackathon.com/HACKATHON.md. Re-read it before submitting.

---

## 1. Locked decisions (do not re-litigate)

| Topic | Decision |
|---|---|
| Name | Project **Drizzle**, token **$DRIZZLE** (symbol `DRIZZLE`, 18 decimals) |
| Tracks | `memecoins`, `games`, `open` |
| Chain | NEAR **testnet** only |
| Oracle | **OutLayer** (`outlayer.testnet`) running a WASI P2 module forked from `github.com/out-layer/weather-example`, fetching **Open-Meteo** (no API key) |
| Contract | Rust, `near-sdk` 5.x, `near-contract-standards` (NEP-141 + storage management), built with `cargo near build` |
| Parameters | Emission **100 DRIZZLE/round** · evaporation **10 bps per dry round** · rain threshold **0.2 mm** · storm cap **5.0 mm** · seed **100 DRIZZLE** once per account · keeper tip **1 DRIZZLE** |
| Cities (fixed order, index 0–11) | 0 Bergen (60.39, 5.32) · 1 São Paulo (−23.55, −46.63) · 2 Lagos (6.52, 3.38) · 3 Singapore (1.35, 103.82) · 4 Tokyo (35.68, 139.69) · 5 Jakarta (−6.20, 106.85) · 6 Mumbai (19.08, 72.88) · 7 London (51.51, −0.13) · 8 Lisbon (38.72, −9.14) · 9 Seattle (47.61, −122.33) · 10 Cairo (30.04, 31.24) · 11 Dubai (25.20, 55.27) |
| Deployments | `drizzle.testnet` (live: 1 round = 1 real hour) and `drizzle-replay.testnet` (replay: 1 round = 60 s, replaying real hours from **2026-09-24T00:00Z**, the 7-day window where 10 of 12 cities got rain) |
| Admin | None. All parameters set in `new()`, no setters. Full-access keys deleted after review |
| Frontend | Next.js (App Router) + TypeScript + `@near-wallet-selector` (testnet), no backend. Reads contract views over RPC and calls Open-Meteo from the browser for Verify |
| Keeper | Node/TS script anyone can run. It calls `tick()` whenever a round is due |

**Open decisions you own:** the exact module layout, the event schema details, map rendering (SVG world map or Leaflet + OSM tiles), the hosting choice to propose to the human, and anything not in this table. Record each decision in `STATUS.md` → Decisions.

---

## 2. System spec

### 2.1 Oracle WASM (`oracle/`)
- Fork `out-layer/weather-example` (MIT/Apache). Target `wasm32-wasip2`.
- **Input** (stdin JSON): `{"hour": <unix_hour u64>, "cities": [[lat_e4, lon_e4], ...12]}` (lat/lon × 10⁴ as i32).
- Fetch: `GET https://api.open-meteo.com/v1/forecast?latitude=<csv>&longitude=<csv>&hourly=precipitation&start_hour=<YYYY-MM-DDTHH:00>&end_hour=<same>&timezone=UTC`. Verified on 2026-11-11: a multi-location query returns a JSON array, one object per city, with `hourly.precipitation[0]`. For hours older than ~90 days, fall back to `https://archive-api.open-meteo.com/v1/archive` (`start_date`/`end_date` + pick the hour).
- **Output** (stdout JSON): `{"hour": <u64>, "mm_tenths": [u16; 12], "source": "open-meteo"}`, where `mm_tenths = round(mm × 10)` and null → 0.
- Errors: non-200, wrong array length or a missing hour → output `{"error": "..."}` and exit non-zero. Never output partial data.
- Must pass `wasmtime` locally with a fixture hour (2026-09-24T14:00) and match a direct `curl` of the same URL.

### 2.2 Contract (`contract/`)

**Config** (`#[init] new(config: Config)`, all immutable):
```
Config {
  name/symbol/decimals/icon (FT metadata),
  cities: Vec<City { name: String, lat_e4: i32, lon_e4: i32 }>,   // exactly 12, order above
  emission_per_round: U128, evap_bps: u16, rain_threshold_tenths: u16, cap_tenths: u16,
  seed_amount: U128, keeper_tip: U128,
  round_seconds: u64,                 // 3600 live, 60 replay
  genesis_ts: u64,                    // unix seconds; round 0 starts here
  first_hour: u64,                    // unix hour of round 0 (live: hour of genesis; replay: 2026-09-24T00 = 497280)
  replay: bool,
  oracle: { outlayer: AccountId, source: <code source JSON pinned by WASM hash>, resource_limits: <JSON>, deposit: U128 }
}
```
Round `r` covers hour `first_hour + r`. `tick()` is allowed when:
- replay: `now ≥ genesis_ts + (r+1)·round_seconds`
- live: `now ≥ (first_hour + r + 1)·3600 + 120` (grace for data availability)

Ticks settle rounds strictly in order, one per call. Catch-up after downtime is allowed.

**State:**
- per-city pool `{ value: u128, shares: u128, pending_total: u128, jackpot: u128, last_rain_round: Option<u64>, last_mm_tenths: u16 }`
- per-user per-city `{ shares, pending, pending_round }`
- `seeded: LookupSet<AccountId>`
- `round_logs: LookupMap<u64, RoundLog>`
- `sprout_ratio: LookupMap<(u64 round, u8 city), (pending_in, shares_out)>`
- `next_round: u64`, `in_flight: Option<(u64 round, u64 started_ts)>`
- totals `minted`, `burned`

**Methods:**
- NEP-141 core + storage management via `near-contract-standards`. FT events must follow NEP-297 (`ft_mint`, `ft_burn`, `ft_transfer`).
- `claim_seed()`: payable. Registers storage if needed, mints `seed_amount` once per account.
- `plant(city: u8, amount: U128)`: moves balance into the city's `pending` (sprouts after round `next_round` settles, i.e. it can't earn in the round it was planted).
- `uproot(city: u8, shares: Option<U128>)`: first settles the user's pending (via `sprout_ratio`), then burns shares and credits `shares × value / total_shares` back to the balance. Unsprouted pending is returned 1:1.
- `rain_dance(city: u8, amount: U128)`: burns from balance (emit `ft_burn`), `jackpot += amount`.
- `tick()`: payable (keeper attaches `oracle.deposit`). Requires the round to be due and no call in flight; if a call has been in flight more than 10 min, allow a retry. Builds the input JSON from config, calls `request_execution` on `outlayer`, then `.then(on_weather(round, keeper))`.
- `on_weather(round, keeper, #[callback_result] ...)` (`#[private]`):
  - **On Err/None or a malformed result** (wrong hour, length ≠ 12): clear `in_flight` and log the failure. Don't advance. The keeper's deposit is refunded by OutLayer to `payer_account_id`.
  - **On success:** let `w_c = min(mm_c, cap)` if `mm_c ≥ threshold`, else 0, and `W = Σw`. For each city:
    - if `w_c > 0` and `shares > 0`: `pot = E·w_c/W + jackpot`; `value += pot`; mint `pot` to the contract; `jackpot = 0`; `last_rain_round = round`
    - if `w_c > 0` and `shares == 0`: mint nothing; the jackpot stays
    - if `w_c == 0`: `burn = value·evap_bps/10000`; `value -= burn`
    - then convert `pending_total` into shares at the post-settlement price (1:1 if the pool is empty) and store `sprout_ratio[(round, city)]` when `pending_total > 0`
  - Mint `keeper_tip` to `keeper`. Store `RoundLog { round, hour, mm_tenths, minted_per_city, burned_per_city, outlayer_request_id (if obtainable), settled_ts }`. Emit a custom NEP-297 event `drizzle_round`. Advance `next_round`.
- **Views:** `get_config`, `get_round_state` (next_round, due_at, in_flight), `get_cities`, `get_user(account_id)` (per-city value incl. pending), `get_round_log(round)`, `get_round_logs(from, limit)`, `get_stats` (total_supply, minted, burned, seeded_count).

**Correctness invariants (unit tests must assert them):**
1. FT `total_supply == Σ balances + contract-held pool value + Σ pending` at all times.
2. No user can earn in the round they planted.
3. Rain on a city with zero shares mints nothing.
4. A dry round reduces `value` by exactly `evap_bps`.
5. Only the `.then` callback path can apply weather; direct calls to `on_weather` fail.
6. Out-of-order or duplicate rounds are rejected.
7. With 12 cities, tick + callback gas stays well under limits, independent of the number of users.

> ⚠️ **Verify against current docs before coding the OutLayer call.** The docs show two argument shapes for `request_execution` (`code_source`/`source`, with or without `payer_account_id`) and wrap JSON results in `OutLayerResponse<T> { success, result, error }`. Use whatever the **current** `outlayer.testnet` contract accepts. Sources: https://app.outlayer.ai/llms-full.txt (sections "NEAR Contract Integration" and "OutLayerResponse Wrapper"), `github.com/out-layer/outlayer/wasi-examples/private-dao-example/dao-contract/src/lib.rs`, and the `weather-example` README. Send a `scout` to extract the exact current signature first (WP-1).

### 2.3 Keeper (`keeper/`)
`node keeper.js --contract drizzle-replay.testnet`. It polls `get_round_state` every 10 s and calls `tick()` with the deposit when a round is due. It logs every result and backs off on failure. Credentials come from env (`NEAR_ACCOUNT_ID`, `NEAR_PRIVATE_KEY`), never from files in the repo. It must run unattended for hours. **Measure the real testnet cost of one tick on Day 1** and budget faucet NEAR for 1 tick/min through Nov 18. If the cost is too high, propose a slower replay (e.g. 1 round per 3 min) to the human.

### 2.4 Frontend (`web/`)
One page:
- **Map** with 12 pins: size = total staked; a blue pulse when the last settled round rained; live "raining now" from Open-Meteo `current=precipitation`.
- **Round bar:** "Round N settles in mm:ss", plus the live/replay toggle (the two contracts).
- **City card:** crowd, your stake (incl. "sprouting"), last rain, jackpot, actions Plant / Uproot / Rain dance.
- **Wallet:** connect, Claim seed, your balance.
- **Feed:** the latest 30 round logs, as human sentences ("São Paulo 2.1 mm → +31 DRIZZLE to planters", "Cairo dry → 0.10% evaporated").
- **Verify drawer** per round: on-chain `mm_tenths`; the OutLayer request ID and how to view its attestation; a browser re-fetch of the same Open-Meteo URL; a ✅ when all 12 match.
- **About:** the mechanic in 5 lines, the parameters from `get_config`, the "no admin keys" proof (show the result of a `view_access_key_list` RPC call), and the supply chart (`docs/supply.svg`).

Mobile-usable. No seed phrases in the normal flow beyond wallet-selector.

### 2.5 Docs (`README.md`, `docs/`)
The README is read **first by Claude Code/Codex screeners**, then by humans. Use the skeleton in `IDEA.md` §7: "Try it in 2 minutes" as numbered **do X → see Y** steps; the mechanic; parameters; simulation results (from `sim_results.txt`, plus `docs/supply.svg` generated by a script from `sim.py`); how NEAR is used (NEP-141, OutLayer `request_execution` + yield/resume, keyless account); the trust model and known limits (sybil = one seed per account; Open-Meteo is the data source; you trust Intel TDX/OutLayer); run-locally steps; "Built at NEAR HACKATHON"; list the AI and libraries used; and a LICENSE (MIT).

---

## 3. Work packages

Rules for every brief you send a worker:
1. **Goal** (one sentence)
2. **Files owned** (only these may be created or edited)
3. **Interface contract** (exact signatures and JSON shapes, copied from §2)
4. **Acceptance command(s)** that must pass
5. **Do-not-touch** list
6. **Report format:** files changed, commands run with output tail, open questions

Keep briefs specific: DeepSeek workers do well with precise specs and drift on vague ones. Run independent WPs in parallel. **You** run every acceptance command yourself before marking a WP done.

| WP | Agent | Depends | Output / acceptance |
|---|---|---|---|
| **WP-0 Setup** | you + human | – | Toolchain check (`rustup` 1.85 + `wasm32-wasip2`, `cargo-near`, `near` CLI (near-cli-rs), `outlayer` CLI, `wasmtime`, node 20+, pnpm). The human creates/funds `drizzle-dev.testnet` and logs in to `outlayer` and `near`. `git init`, `.gitignore`, `STATUS.md` |
| **WP-1 Doc scout** | `scout` | – | `docs/notes/outlayer.md`: the exact current `request_execution` args, callback result type, how to get the request ID and attestation link, `outlayer upload/deploy` commands, deposit guidance. Cite each with a URL or file path. Also confirm the Open-Meteo `start_hour`/`end_hour` behaviour |
| **WP-2 Oracle** | `task` | WP-1 | `oracle/` builds; `wasmtime` with the fixture input prints 12 values equal to a direct `curl`; error cases tested |
| **WP-3 Oracle live** | you | WP-2, WP-0 | Upload + deploy to OutLayer testnet; one manual `near call outlayer.testnet request_execution …` returns the correct JSON. **Record the cost.** ⛔ Gate: don't start WP-5 until this passes |
| **WP-4 Pool math** | `task` | – | `contract/src/pool.rs`: pure functions (settle round, sprout, uproot value, evaporation) + unit tests for invariants 1–4, with no near-sdk deps. Cross-check: port 48 h of `rain_60d.json` into a test and compare against a tiny Python reference (`tools/ref_settle.py`) |
| **WP-5 Contract** | `task` | WP-1, WP-4, WP-3 | `contract/` with the FT, methods, views, events, OutLayer call + callback; `cargo near build` passes; unit + `near-workspaces` sandbox tests pass (mock the OutLayer callback in the sandbox) |
| **WP-6 Deploy replay** | you | WP-5 | Deploy `drizzle-replay.testnet`; run the keeper by hand for 5 rounds; check the logs, supply invariant and events on the explorer |
| **WP-7 Keeper** | `sonic` or `task` | WP-5 interface | `keeper/` runs unattended for 30 min against the replay contract with no errors |
| **WP-8 Frontend** | `task` (split 8a data layer / 8b UI if slow) | WP-5 views | `pnpm build` passes; against the replay contract you can connect, claim, plant, see sprouting → earning, rain dance, Verify ✅, all on a 390 px wide screen |
| **WP-9 Review** | `reviewer` + `security-reviewer` in parallel | WP-5 | Findings on the contract (arithmetic, rounding, callback auth, storage, in-flight lock, replayability). You triage and fix. Re-review if anything material changed |
| **WP-10 Docs** | `task` | WP-6 | README, `docs/supply.svg` (from `sim.py`), LICENSE; README steps work verbatim on a clean clone (you verify) |
| **WP-11 Finalize** | you + human | WP-6…10 | Deploy live `drizzle.testnet`; **human confirms → delete full-access keys** on both contracts and show `list-keys` empty; human hosts the frontend + keeper (through Nov 18); final README pass |
| **WP-12 Launch** | you + human | WP-11 | 75 s video script (`IDEA.md` §5), X post draft (`IDEA.md` §8, rename to Drizzle, tag @NEARProtocol #NEARHackathon @NearBuilders @NearLegion @out_layer), submission payload printed for human confirmation |

**Critical path:** WP-1 → WP-2 → WP-3 → WP-5 → WP-6 → WP-11. Start WP-4 and the WP-8 UI shell with mocked data in parallel on Day 1.

**Day plan:**
- **Day 1 (Nov 12):** WP-0…WP-6. Exit criterion: the replay contract settles real rounds via OutLayer.
- **Day 2 (Nov 13):** WP-7…WP-10, then the video.
- **Nov 14:** WP-11/12 and buffer. Submit by ~18:00 UTC, not 23:59.

---

## 4. Cut list (when behind schedule)

Cut in this order:
1. Live `drizzle.testnet` (ship replay only, and say so)
2. Map polish (use a city list)
3. Mobile polish
4. The `archive-api` fallback

**Never cut:** the real OutLayer fetch, the keyless contract, the Verify drawer, the replay demo, the README "do X → see Y" steps.

Stretch (only if Day 1 exits early): ⚡ thunderstorm hour (WMO 95–99) → OutLayer VRF picks one planter for a 10× bonus; a bot-proof `claim_seed` via OutLayer `captcha-example`.

---

## 5. STATUS.md protocol

Keep `STATUS.md` at the repo root, and update it after every WP state change. It has these sections:
- **Now:** the active WPs and their assigned agents
- **Done:** WP list, each with its acceptance evidence (a command and an output excerpt)
- **Decisions:** what was decided, with a one-line reason
- **Blockers / waiting on human**
- **Costs:** NEAR spent per tick, the faucet balance

If the session is compacted or restarted, re-read `HANDOFF.md` + `STATUS.md` before doing anything else.

---

## 6. Definition of done

- [ ] Public repo (human), MIT LICENSE, README verified on a clean clone
- [ ] `drizzle-replay.testnet` settling rounds via OutLayer with the keeper running; full-access keys deleted
- [ ] Hosted frontend: claim → plant → earn → Verify ✅ works for a stranger
- [ ] `docs/supply.svg` + simulation section in the README
- [ ] 75 s video + X post published by the human
- [ ] Submission payload confirmed by the human and sent (201), with tracks `["memecoins","games","open"]`

**First message to the human:** confirm the pre-approval scope (§0), list the WP-0 items they must do (accounts, logins, faucet), and state that you are dispatching WP-1 (scout) and WP-4 (pool math) in parallel now.
