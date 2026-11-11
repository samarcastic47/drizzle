# Manual test guide (testnet)


Running services (in the omp session, persistent): `drizzle-web` (http://localhost:4321), `drizzle-keeper` (replay, every 10 s), `drizzle-keeper-live` (live, every 30 s). Logs: `/tmp/keeper-replay.log`, `/tmp/keeper-live.log`. Browser reads use `https://test.rpc.fastnear.com` (`rpc.testnet.near.org` is deprecated and rate-limits).
The site runs locally at **http://localhost:4321** (a static build pointed at NEAR testnet RPC).
Contracts: `drizzle-replay.testnet` (1 round = 60 s, replays 2026-09-24 … 09-30 in a loop) and `drizzle-live.testnet` (1 round = 1 real hour).
A keeper (`drizzle-dev.testnet`) ticks the replay contract automatically, so a round settles about every minute.

Use a testnet wallet that is **not** drizzle-dev, e.g. a fresh account in MyNearWallet testnet (https://testnet.mynearwallet.com) or Meteor Wallet switched to testnet. It needs ~0.1 testnet NEAR for the storage deposit and gas (new accounts get free NEAR).

## Flow: do X → see Y

| # | Do | See |
|---|---|---|
| 1 | Open http://localhost:4321 | Header "Drizzle $DRIZZLE", a **Replay** tab selected, "Round N settles in m:ss" counting down, a world map with 12 pins, a city list below it, the **Round feed** filling with settled rounds |
| 2 | Click **Connect wallet**, pick MyNearWallet or Meteor, approve | Your account name in the header, balance 0 DRIZZLE, a **Claim seed** button with "≈0.032 NEAR storage deposit (refundable)" |
| 3 | Click **Claim seed**, approve in the wallet (attaches ~0.032 NEAR) | Balance **100 DRIZZLE**; the Claim seed button disappears. A second claim is impossible ("already seeded") |
| 4 | Click a city pin or a city in the list, e.g. **São Paulo** | The **Selected city** card: planters, staked, jackpot, last rain, your position, the amount field with "min 1 DRIZZLE" |
| 5 | Enter **50**, click **Plant**, approve (1 yocto) | Balance 50; the card shows "sprouting 50 · after round N" (it can't earn in the round you planted) |
| 6 | Wait for the countdown (≤ 60 s) plus ~20–40 s while "oracle running in OutLayer TEE…" shows | The feed gets a new round block: rainy cities "City X.X mm → +N DRIZZLE to planters", dry cities "N cities dry → 0.10% evaporated". Your São Paulo stake now shows as planted (value), not sprouting |
| 7 | Watch a few more rounds | Your stake value goes up on rainy São Paulo hours and down 0.10% on dry ones. The 2026-09-24 week had rain in São Paulo, Lagos, Singapore, Jakarta, Mumbai, … |
| 8 | Select **Dubai**, enter **10**, click **Rain dance**, approve | Balance −10; Dubai's jackpot +10 (burned until it rains in Dubai; it doesn't rain there this week) |
| 9 | In the feed, click **Verify** on any round | A drawer: 12 rows of on-chain mm next to a live browser re-fetch of the same Open-Meteo URL, and "✅ all 12 match". Below: the OutLayer request id, the pinned WASM hash `7d708ee7…128b`, and the tick block. The attestation link may fall back to the OutLayer explorer (the OutLayer API blocks cross-origin lookups); the README explains the curl check |
| 10 | Select São Paulo, click **Uproot all**, approve | The stake returns to your balance at its current value (more or less than 50 depending on rain/evaporation) |
| 11 | Scroll to **About** | The mechanic in 5 lines, the parameter table from `get_config`, "access keys" for the contract, stats, and the 60-day supply chart |
| 12 | Click the **Live** tab | The live instance: 1 round per real hour. The countdown runs to the next hour + 2 min. Once the hour closes, Plant/Uproot are disabled until it settles ("round closing") |
| 13 (optional) | When a round is overdue and no keeper ticked, click **Tick (+1 DRIZZLE)** | Your wallet pays the OutLayer deposit (0.01 NEAR, mostly refunded) and you get 1 DRIZZLE when the round settles |

## Expected rough edges
- Access keys: keys are **not deleted yet** (that's the last step, after your test), so About lists 1 key. The final version shows 0.
- A plant below 1 DRIZZLE, or too small for a city's share price, is blocked with a message.
- If a round shows "round failed" in the feed, OutLayer returned no data; the next tick retries the same round.

## Checks from a terminal
```bash
near contract call-function as-read-only drizzle-replay.testnet get_round_state json-args {} network-config testnet now
near contract call-function as-read-only drizzle-replay.testnet get_stats json-args {} network-config testnet now
near account list-keys drizzle-replay.testnet network-config testnet now
```

## Stop / restart
- Keepers: `pkill -f "keeper.js --contract drizzle-"` (stops both). Each tick costs ~0.0024 NEAR from drizzle-dev.
- Web server: `pkill -f "serve out -l 4321"`; restart with `cd web && pnpm build && pnpm start`.
