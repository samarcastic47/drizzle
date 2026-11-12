# Drizzle ($DRIZZLE): the memecoin printed by the sky ☔

A NEP-141 token on NEAR that mints only when it actually rains in 12 real cities. Each round's rainfall is fetched inside an Intel TDX enclave on [OutLayer](https://outlayer.ai) and delivered to the contract on-chain. Anyone can re-check a round against Open-Meteo.

**Live demo:** https://drizzle-wine.vercel.app · **Contracts:** `drizzle-replay.testnet` (1 round = 60 s, replaying a real week) · `drizzle-live.testnet` (1 round = 1 hour) · NEAR testnet

## Try it (2 minutes)

1. Open https://drizzle-wine.vercel.app → 12 city plots, each with last round's weather and a crop sized by how much is staked.
2. **Connect wallet** (any testnet wallet) → **Claim 100 seeds** → balance shows 100 DRIZZLE.
3. Pick a plot → **Plant 50 seeds** → the plot shows "sprouting".
4. Wait one round (~60–90 s) → rain falls on wet plots with gold **+N** coins; dry plots show **−0.1%**.
5. **Rounds & rules → Verify** on a round → on-chain readings vs. a fresh Open-Meteo fetch in your browser: ✅ all 12 match.

## How it works

| Action | Effect |
|---|---|
| Claim seed | 100 DRIZZLE once per account |
| Plant | Stake in a city. It only earns from the *next* round, so you can't snipe rain you've already seen |
| Rain | Each round mints 100 DRIZZLE, split across cities with ≥ 0.2 mm (weighted by `min(mm, 5)`), then pro-rata to planters. Crowded cities pay little, empty wet ones pay a lot |
| Dry round | The city's stake evaporates 0.10% (burned) |
| Rain dance | Burn coins into a city's jackpot, paid to its planters on the next rain |
| Tick | Anyone settles a due round and earns 1 DRIZZLE |

**Why the weather can't be faked:** `tick()` calls OutLayer `request_execution` with a WASM pinned by sha256 (`7d708ee7…128b`), which the enclave checks before running. The result comes back through yield/resume to a `#[private]` callback that validates the hour and all 12 readings. Parameters are fixed in `new()` and have no setters.

**Nobody can change the rules:** both contract accounts have zero access keys, so the code and parameters can never be upgraded, not even by us. Check it yourself:

```bash
near account list-keys drizzle-replay.testnet network-config testnet now   # → empty
near account list-keys drizzle-live.testnet network-config testnet now     # → empty
```

## Simulation: 60 days of real weather

`sim.py` replays 1,440 real hours for 1,000 players. At 100/round emission and 0.10% evaporation, supply stays flat: **110,314** after 60 days, from 100,000. Herding into the wettest city loses money (47 per player); loyal planters end at 325.

![Supply over 60 days](docs/supply.svg)

## Run locally

```bash
cd oracle && cargo test && ./build.sh && ./verify.sh      # WASI oracle (Rust 1.85, wasm32-wasip2)
cd contract && cargo test && cargo near build non-reproducible-wasm
cd keeper && pnpm i && pnpm build && NEAR_ACCOUNT_ID=… NEAR_PRIVATE_KEY=… pnpm start -- --contract drizzle-replay.testnet
cd web && pnpm i && NEXT_PUBLIC_MOCK=1 pnpm dev           # mock mode, no chain needed
```

Built with near-sdk, near-api-js and wallet-selector. The oracle is forked from OutLayer's weather-example (MIT/Apache-2.0). Weather data: [Open-Meteo](https://open-meteo.com) (CC BY 4.0). License: [MIT](LICENSE).
