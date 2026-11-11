# Drizzle: the memecoin printed by the sky

> **$DRIZZLE** only mints when it actually rains somewhere real. Plant your coins in a city. When it rains there, you get a share of the new coins. When it's dry, your stake evaporates. Every rain reading is fetched inside an Intel TDX enclave on OutLayer and delivered to the contract on-chain. The contract account has no keys, so nobody can fake the weather or change the rules, including the dev.

Tracks: **`memecoins`** (34 entries, ~8.8% base odds of a top-3 prize) · **`games`** (39, ~7.7%) · **`open`**
Build time: **2 days, solo**. Testnet only, no mainnet money needed.

---

## 1. Why this wins (evidence)

| Signal | What it means for us |
|---|---|
| The memecoin track prompt says *"Judges score the mechanic… the mechanic in the contract, not in a backend… no admin key… simulate 60 days and show supply over time"* | We hit every item. The mechanic is in-contract, the parameters are immutable, the account is keyless, and we have a **real-data 60-day simulation** (§6). |
| 34 memecoin entries (STAG, LIFO, BEFEWER, Meme Wars, Tripwire, Anyone Can Rug…) | **Every** one is driven by internal game theory. **None reads the real world.** We're the only coin whose supply follows an external, verifiable signal. |
| Vadim (OutLayer founder) listed *"a memecoin that mints when it rains on Thursday"* as idea #5 of 10 ([post](https://x.com/zacodil/status/2117283219364016192)) | A partner asked for exactly this, and nobody in 527 submissions has built it (checked Nov 11). OutLayer ships a ready `weather-example` (updated Nov 4). |
| Prior art: **RainCoin** at ETHGlobal Brussels ([showcase](https://ethglobal.com/showcase/raincoin-tn2tm)) won no prize | It was "trust our backend's OpenWeather call" with no game. Our difference is a verifiable TEE oracle, a market between players, and rituals. |
| Illia is currently pushing verifiable private compute ("only on near_ai can you validate…") | The "click Verify, the enclave proof and the raw sky data match" beat speaks directly to that. |

**Ticker note:** `$RAIN` is a large Arbitrum options-protocol token (billions in market cap) ([CoinGecko](https://www.coingecko.com/sv/coins/rain)), so we don't use it. The coin is **Drizzle ($DRIZZLE)**.

---

## 2. The mechanic (one screen)

**12 cities**, chosen for climate spread: Bergen, São Paulo, Lagos, Singapore, Tokyo, Jakarta, Mumbai, London, Lisbon, Seattle, Cairo, Dubai.

| Verb | What happens | Why it's there |
|---|---|---|
| **Claim seed** | Each NEAR account claims 100 $DRIZZLE once | Bootstraps the game. Sybil limits are stated in the README |
| **Plant** | Stake $DRIZZLE in a city. It **sprouts at the next round** (it can't earn in the round you planted) | Anti-snipe: you can't plant after you see rain |
| **Rain** (each round = 1 real hour) | Hourly emission **E = 100** is split across cities that rained (≥0.2 mm), weighted by `min(mm, 5)`. Each city's share is split **pro rata among its planters** | **Crowded wet cities pay little, empty wet cities pay a lot.** That turns it into a forecasting market, not "everyone stakes in Singapore" |
| **Evaporate** | In a dry hour, a city's planted stake loses **0.10%**, burned | Supply sink. Holding a dry bet costs you |
| **Rain dance** (ritual) | Burn $DRIZZLE into a city's **cloud jackpot**. It pays out to that city's planters the next time it rains | The on-chain ritual the track hint asks for. Dubai and Cairo got no rain in 60 days, so their jackpots become community lore ("Dubai: 4,200 $DRIZZLE danced, last rain: never") |
| **Uproot** | Withdraw your stake (current value) | No lockup. Moving costs you the next round's sprout delay |
| **Tick** | Anyone can call it once the round is over. It requests the reading from OutLayer and pays the caller 1 $DRIZZLE | No privileged keeper |

**Parameters** (E, evaporation rate, thresholds, cities, round length) are set in `new()` and readable via `get_config()`. There is no setter, and the account's full-access keys are deleted after deploy.

### The game loop
- **Every 30s:** watch the live rain map, check the forecast, eye the crowd per city.
- **Every hour:** a round settles. Mint and evaporation events land in the feed.
- **Every session:** move stake ahead of a forecast front, or join a rain dance.

---

## 3. Architecture

```
 Browser (Next.js)                 drizzle.testnet (Rust, near-sdk 5)          outlayer.testnet
 ───────────────                   ────────────────────────────────────          ─────────────────
 map + plant/uproot ──────────────► plant / uproot / rain_dance / claim_seed
 keeper (cron/GitHub Action) ─────► tick(round) ──request_execution(code=WasmHash,─► TDX worker runs
                                                   input={round_hour, cities})       rain-oracle.wasm
                                                                                       │ HTTPS → Open-Meteo
                                    on_weather(#[private] callback) ◄─yield/resume─────┘ (signed result)
                                    • for each city: mint or evaporate
                                    • store RoundLog {hour, readings[], outlayer_request_id}
 "Verify" button ── re-fetches the same hour from Open-Meteo in the browser and compares it with the on-chain readings
```

### Why the weather can't be faked
1. **The contract builds the input** (`round_hour`, city coordinates). The keeper can't pick the hour or the cities.
2. **The code is pinned** by `WasmHash` in the contract's immutable config. OutLayer attests that exactly that binary ran (Intel TDX).
3. **Only `outlayer.testnet` can resolve the promise.** `on_weather` is `#[private]` and only reachable as the `.then()` of our own `request_execution` call.
4. **Anyone can re-check** using Open-Meteo hourly history (`/v1/forecast?past_days=…&hourly=precipitation`). It needs no key and the data is reproducible.
5. **The account is keyless** after deploy, so the parameters and code are frozen.

Write the residual trust honestly in the README: you trust Intel TDX, OutLayer's worker set, and Open-Meteo's data (stretch: cross-check with a second source, OpenWeather with its key sealed in OutLayer).

### Contract state: lazy accounting, O(cities) per tick
Each city is a pool `{ value: u128, shares: u128, pending: u128, jackpot: u128 }`. A user holds `shares[city]`; their stake is `shares × value / total_shares`.
- **Rain:** `value += city_emission + jackpot`, mint to the contract, reset the jackpot.
- **Dry:** `burn = value × 0.001`, `value -= burn`.
- **Plant:** goes to `pending`. At the next tick, pending converts to shares at the post-settlement price. That's the anti-snipe rule.
- Tick gas doesn't grow with the number of players.

### The oracle WASM (fork `out-layer/weather-example`)
Input `{"hour":"2026-11-12T14:00","cities":[[lat,lon],…]}` → one batched Open-Meteo call → output `{"hour":…,"mm":[0.0,1.4,…],"source":"open-meteo"}`. Round the values to 0.1 mm and convert them to integers before returning, so the contract stores `u16` tenths of a mm.

### Demo mode: replay
`new(..., replay_start: Option<hour>, round_seconds)`. A second deployment, **drizzle-replay.testnet**, runs one round per minute and maps round *n* to real hour `replay_start + n`. The judges watch a real past week of weather settle in minutes, with the same contract and the same TEE path. Production uses live hours.

---

## 4. What you build in 2 days

### Day 1: the core works end to end on testnet
| Block | Task | Done when |
|---|---|---|
| 1h | `outlayer login testnet`, fork `weather-example` → `rain-oracle` (Open-Meteo batch, no key), `wasmtime` local test | `echo '{…}' \| wasmtime rain-oracle.wasm` prints 12 readings |
| 1h | `outlayer upload` + `deploy`; manual `near call outlayer.testnet request_execution … --deposit 0.1` | A real attested result on testnet |
| 4h | Contract: NEP-141 (`near-contract-standards`), city pools, claim/plant/uproot/rain_dance, `tick` → `request_execution` → `on_weather`, `RoundLog`, `get_config`, events (`mint`, `evaporate`, `dance`) | Unit tests: rain mints, dry burns, pending sprouts next round, no-planter rain mints nothing |
| 2h | Deploy `drizzle-replay.testnet` (round 60s), keeper script (`node keeper.js` loop), **delete full-access keys** | 20 replay rounds settle unattended |

### Day 2: make it look like a product
| Block | Task |
|---|---|
| 4h | Next.js page: world map with 12 city pins (size = crowd, blue pulse = raining), live Open-Meteo overlay, "next round in mm:ss", plant/uproot/dance via NEAR Wallet Selector, live event feed, per-city "last rain" and jackpot |
| 1h | **Verify drawer** on each round: on-chain readings, OutLayer request ID and attestation, browser re-fetch from Open-Meteo, green check when they match |
| 1h | Supply chart from `sim.py` plus the live supply from the contract |
| 2h | README (below), 75s video, X post, submit |

**Stretch (only if Day 1 finishes early):** ⚡ *Lightning*: a thunderstorm hour (WMO code 95–99) triggers OutLayer **VRF** to pick one planter in that city for a 10× bonus. 🤖 *Bot-proof seed*: gate `claim_seed` with OutLayer's `captcha-example`.

**Cut first if time runs out:** the second data source, lightning, the captcha, the map polish (a city list works too).
**Never cut:** the real TEE fetch, the keyless account, the Verify button, the replay demo.

---

## 5. Demo script (75 seconds)

1. **(0–8s) Hook.** "Most memecoins are printed by devs. This one is printed by the sky." A live map: Lisbon raining, Dubai dry.
2. **(8–20s)** Claim 100 $DRIZZLE. The forecast shows a front over São Paulo, which has few planters. Plant there.
3. **(20–40s)** Replay mode: rounds tick. The feed reads *"São Paulo 2.1 mm → +31 $DRIZZLE to 4 planters"*, *"Cairo dry → 0.10% evaporated"*. The balance goes up.
4. **(40–52s)** Click **Verify**: the enclave attestation, the code hash and the raw sky data in the browser all match. "Nobody, including me, can fake a rainy hour."
5. **(52–62s)** The Dubai rain dance: "4,200 $DRIZZLE burned to the clouds. Last rain: never." Add 50.
6. **(62–75s)** The terminal shows `near account list-keys drizzle.testnet` → *no keys*. A 60-day supply chart on real weather. "The joke has rules, and the rules are verifiable."

---

## 6. Simulation on real weather (the track prompt asks for this)

`sim.py` replays **1,440 real hours** (Sep 12 – Nov 10, 2026) for the 12 cities (`rain_60d.json`, Open-Meteo) with 1,000 players × 100 seed. Full output is in `sim_results.txt`.

**Chosen parameters: E = 100/h, evaporation 0.10% per dry hour.** In the mixed scenario, supply stays flat (weekly: 106.7k → 112.2k → 111.9k → 111.0k → 111.4k → 109.4k → 112.0k → 108.8k). There's no runaway inflation and no death spiral, with no admin intervention.

| Scenario (E=100, evap 0.10%) | Supply after 60d | Per-player result |
|---|---:|---|
| Everyone stays in a random city | 98.0k | avg 98 |
| 80% herd chase the wettest city | 102.8k | **herd 47** vs loyal 325 |
| Mixed: 40% forecaster, 30% herd, 20% loyal, 10% idle | 110.3k | forecaster 98 · **herd 62** · loyal 213 · idle 100 |

Findings worth quoting in the README:
- **Herding is punished.** Chasing the crowd's favourite city halves your stack, because your rain is shared with everyone else.
- **Patience in under-crowded wet cities wins.** For a solo planter against equal crowds: São Paulo 100 → 305, Lagos → 271, Bergen → 246, while Cairo and Dubai drop to 24 (60 dry days).
- **Rain dances are a pure sink until it rains.** 200/day burned into Dubai took 126k out of circulation over 60 days, so the ritual is deflationary by construction. Dubai's rainy season runs Nov–Mar, so the lore writes itself.
- With 0.25% evaporation, supply collapses ~50%, and with 0.05% it inflates ~50%. The chosen value sits in the stable band.

---

## 7. README skeleton (Claude Code and Codex screen this first)

```
# Drizzle ($DRIZZLE): the memecoin printed by the sky
One-line: NEP-141 whose supply mints only when it rains in real cities, read by an attested OutLayer TEE oracle.

## Try it in 2 minutes (testnet, no funds)
1. Open https://<demo-url> → Connect a testnet wallet → "Claim seed" → you see 100 DRIZZLE.
2. Pick a city with a rain forecast → "Plant 50" → it shows "sprouts next round".
3. Replay instance ticks every 60s → the feed shows mint/evaporate events → your stake changes.
4. Click "Verify" on any round → on-chain readings == Open-Meteo == attested code hash.
5. `near account list-keys drizzle.testnet` → no keys.

## Mechanic / Parameters (immutable, from get_config) / Simulation (sim.py, real data)
## How NEAR is used: NEP-141 · OutLayer request_execution + yield/resume · keyless account
## Trust model and known limits (sybil: one seed per account; Open-Meteo is the data source; TDX trust)
## Run locally: cargo near build · outlayer upload/deploy · node keeper.js · pnpm dev
## Built at NEAR HACKATHON (2026-11-11 → 14). AI-assisted; libraries: near-sdk, near-contract-standards, OutLayer weather-example (MIT/Apache)
```

---

## 8. X post draft

> Most memecoins are printed by devs. **$DRIZZLE is printed by the sky.** ☔
>
> Plant it in a city. When it actually rains there, you get a share of the new coins. When it's dry, your stake evaporates. Crowd into Singapore and your rain is shared with everyone else.
>
> The weather is read inside an @out_layer TEE and delivered on-chain. The contract has no keys, so nobody can fake a rainy hour, not even me.
>
> Built at NEAR HACKATHON @NEARProtocol #NEARHackathon @NearBuilders @NearLegion
> Dubai has been rain-danced 4,200 $DRIZZLE and hasn't rained once. Want to add to it? [demo] [repo]

(Attach the 75s video. Lead with the map, not the code.)

---

## 9. Risks and mitigations

| Risk | Mitigation |
|---|---|
| OutLayer testnet hiccups or callback deserialization errors | De-risk in Day 1 block 2 with a manual `near call`. The docs have a known fix: wrap the result in `OutLayerResponse<T>`. Keep a recorded happy-path video |
| yield/resume latency makes the demo slow | The replay instance plus a pre-recorded fast-forward. Show one live tick, not ten |
| "It's just a gamified oracle" | Lead with the market dynamic (herd loses, patience wins) and the sim numbers. That's the mechanic judges score |
| Sybil farming of the seed | Stated plainly. Stretch: captcha via OutLayer. On mainnet the seed would be replaced by buying in |
| Legal optics ("weather betting") | No real money on testnet, no cash entry, seed-only. Frame it as a game and say so |
| Open-Meteo revises an hour later | Store the readings as delivered and attested. Verify shows "matched at settlement" and explains possible later revisions |

---

## Files here
- `IDEA.md`: this document
- `sim.py`: supply simulator (`python3 sim.py rain_60d.json`)
- `rain_60d.json`: 60 days of hourly precipitation, 12 cities, Open-Meteo (pulled 2026-11-11)
- `sim_results.txt`: full simulation output across 9 parameter sets × 3 scenarios
