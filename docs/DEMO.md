# Drizzle demo script (≈75 s)

**Setup before recording**
- Open http://localhost:4321 (Replay tab) in a 1440×900 window, already zoomed so all 12 plots and the action card fit.
- Use a fresh testnet wallet with ~0.1 NEAR, **not yet connected**. Log in to the wallet in another tab beforehand so the popup is one click.
- Have the round countdown at ~0:40 when you start, so a round settles during step 3.
- Have a terminal ready with: `near account list-keys drizzle-replay.testnet network-config testnet now`
- Recording: screen + mic, no music under the voice.

| Time | On screen | Say |
|---|---|---|
| 0–8 s | The farm: 12 city plots, rain clouds over a few, crops of different sizes | "Most memecoins are printed by devs. **Drizzle is printed by the sky.** Each plot is a real city." |
| 8–20 s | Click **Connect wallet** → **Claim 100 seeds**. Balance shows 100 | "Connect, claim a hundred seed coins. One per account." |
| 20–32 s | Click **São Paulo** (few planters, rain cloud) → chip **50** → **Plant 50 seeds**. The seed arcs in and a seedling pops up with "sprouting" | "I plant where it's raining but the crowd is small. My seed sprouts next round, so I can't snipe rain I've already seen." |
| 32–45 s | Countdown hits 0 → "oracle reading the sky…" → rain falls on wet plots, gold **+N** coins pop, dry plots droop **−0.1%** | "Every round, an OutLayer TEE enclave fetches real rainfall for all 12 cities. Rain mints new coins to that city's planters. Dry cities evaporate." |
| 45–57 s | Open **Rounds & rules** → **Verify** on the round that just settled → "✅ all 12 match", request ID, pinned WASM hash | "Don't trust me. Verify re-fetches the same hour from Open-Meteo right in your browser. All 12 match the on-chain readings, and the code hash is pinned in the contract." |
| 57–65 s | Back on the farm: select **Dubai**, show its jackpot → **Rain dance 10** | "Rain dances burn coins into a city's jackpot. Dubai hasn't seen rain all week." |
| 65–75 s | Terminal: `list-keys` → empty. Cut to the supply chart in How it works | "The contract has no keys: nobody can change the rules or fake a rainy hour, including me. Sixty days of real weather keep supply stable. **The joke has rules, and the rules are verifiable.**" |

**Before the real take**
- `list-keys` only prints *empty* after the full-access keys are deleted (the final step, after your OK). Until then, skip that beat or say "keys get deleted at launch".
- If no round settles in the window, keep talking over step 4. The keeper ticks about every 70–90 s.
- Re-take if the wallet popup covers the plot during the plant animation. Position the popup on the left.

**Backup plan:** if testnet or OutLayer is slow on the day, record with the mock build (`cd web && NEXT_PUBLIC_MOCK=1 pnpm build && npx serve out -l 4322`). It runs the same UI and animations on the real Sep 24–30 weather. Say "replay" rather than "live" in that take.
