# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

Primary: NEAR HACKATHON judges watching a ~75 s demo, and strangers who open the link once and try it with a fresh testnet wallet. The screen has to read in about five seconds. Returning daily players come second.

## Product Purpose

Drizzle ($DRIZZLE) is a NEP-141 memecoin on NEAR testnet whose supply mints only when it actually rains in 12 real cities. Players plant coins in a city. Rain there pays them a share of the round's emission; dry rounds evaporate 0.10% of the stake. Success: a visitor connects, claims the seed, plants in a city, watches a round settle, and verifies the reading, without needing an explanation.

## Positioning

It's the only memecoin whose supply follows a verifiable real-world signal. Each rain reading is fetched by a pinned WASM inside an Intel TDX enclave on OutLayer and delivered to an immutable, keyless contract. Anyone can re-check a round against Open-Meteo in the browser.

## Operating Context

- Two contracts: `drizzle-replay.testnet` (one 60 s round replays one real hour from the week of 24–30 Sep 2026, looping) and `drizzle-live.testnet` (one round = one real hour).
- Wallet via NEAR wallet-selector (MyNearWallet, Meteor, HERE).
- Rounds settle about every 70–90 s in replay, through tick → OutLayer → callback.
- Shown in a screen-recorded demo and on judges' laptops and phones.

## Capabilities and Constraints

- Verbs: claim seed (100 DRIZZLE, once per account, ~0.032 NEAR refundable storage deposit), plant (minimum 1 DRIZZLE, sprouts after the next settlement), uproot all, rain dance (burn into a city's jackpot), tick (anyone, +1 DRIZZLE).
- Views: config, round state, cities, user, round logs (newest 240), stats, access keys.
- Static Next.js export with no backend. Reads go to `test.rpc.fastnear.com`; Verify fetches Open-Meteo from the browser.
- The OutLayer job API blocks cross-origin requests, so the attestation link falls back to the OutLayer explorer.
- City order and names are fixed: Bergen, São Paulo, Lagos, Singapore, Tokyo, Jakarta, Mumbai, London, Lisbon, Seattle, Cairo, Dubai.

## Brand Commitments

- Name "Drizzle", ticker $DRIZZLE, tagline "the memecoin printed by the sky".
- Tone: playful meme-game, where weather is a toy and seeds sprout, but numbers stay exact.
- Dark theme (confirmed by the user).

## Evidence on Hand

- Real Open-Meteo data, `rain_60d.json`. 60-day simulation, `sim_results.txt` and `docs/supply.svg`: supply ends at 110,314 with 0.10% evaporation.
- Live testnet rounds with OutLayer request IDs, e.g. attestation job 57705.
- No users, testimonials or market data. Never invent them.

## Product Principles

1. The mechanic is visible: rain and evaporation must be seen happening, not just reported in text.
2. One obvious next action at a time: connect → claim → pick a city → plant → watch → verify.
3. Trust is demonstrable: every settled round can be verified in one click.
4. Exact numbers, light mood.

## Accessibility & Inclusion

Must work at 390 px wide. Respect `prefers-reduced-motion` for all rain and sprout animation.
