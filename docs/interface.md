# Drizzle contract interface (shared contract for contract / keeper / web)

All `U128` values are JSON decimal strings. All timestamps are unix **seconds** (u64 JSON numbers). City index `city: u8` is 0–11 in the fixed order (Bergen … Dubai).

## Change methods
| Method | Deposit | Args | Notes |
|---|---|---|---|
| `claim_seed()` | `storage_balance_bounds().min` (0.01938 NEAR) if unregistered, else 0; excess refunded | – | Registers storage if needed; mints `seed_amount` once per account, ever (unregister doesn't reset it); panics `"already seeded"` |
| `plant(city, amount)` | 1 yocto | `{ "city": u8, "amount": U128 }` | Moves balance → user pending; sprouts after round `next_round` settles |
| `uproot(city, shares)` | 1 yocto | `{ "city": u8, "shares": U128 \| null }` | `null` = everything (all shares + unsprouted pending). Returns U128 credited |
| `rain_dance(city, amount)` | 1 yocto | `{ "city": u8, "amount": U128 }` | Burns from balance, adds to jackpot |
| `tick()` | `oracle.deposit` | – | Anyone. Returns promise; panics `"round not due"` / `"tick in flight"` |
| `on_weather(round, keeper)` | – | private callback | Not callable directly |
| `new(config)` | – | `{ "config": {...} }` (`contract/scripts/init_args.py`) | `#[init] #[private]`, once |
| NEP-141 / NEP-145 / NEP-148 | standard | standard | `ft_transfer`, `ft_transfer_call`, `storage_deposit`, `storage_unregister` (requires all cities uprooted; `force` burns the balance), `ft_metadata`, … |

## Views
```ts
get_config(): {
  name: string, symbol: string, decimals: number, icon: string | null,
  cities: { name: string, lat_e4: number, lon_e4: number }[],
  emission_per_round: U128, evap_bps: number, rain_threshold_tenths: number, cap_tenths: number,
  seed_amount: U128, keeper_tip: U128,
  round_seconds: number, genesis_ts: number, first_hour: number, replay: boolean,
  replay_hours: number,   // replay: real hours loop over [first_hour, first_hour+replay_hours); 0 = live
  oracle: { outlayer: string, source: object, resource_limits: object, deposit: U128 }
}
get_round_state(): {
  next_round: number,        // round to settle next
  next_hour: number,         // unix hour of next_round (looped in replay, see replay_hours)
  oldest_round_log: number,  // oldest round still stored (next_round - 240, or 0)
  due_at: number,            // unix secs when tick() becomes allowed
  in_flight: { round: number, started_ts: number } | null,
  now: number                // block timestamp, secs
}
get_cities(): {
  index: number, name: string, lat_e4: number, lon_e4: number,
  value: U128, shares: U128, pending_total: U128, jackpot: U128,
  planters: number,          // accounts with shares > 0 or pending > 0 in this city
  last_rain_round: number | null, last_mm_tenths: number
}[]
get_user({ account_id }): {
  balance: U128, seeded: boolean,
  cities: {
    index: number,
    shares: U128,            // incl. pending already sprouted (settled lazily)
    value: U128,             // shares × value / total_shares
    pending: U128,           // not yet sprouted (planted for round >= next_round)
    pending_round: number | null
  }[]                        // always 12 entries
}
get_round_log({ round }): RoundLog | null       // null also for pruned rounds (< oldest_round_log)
get_round_logs({ from, limit }): RoundLog[]     // ascending by round, limit ≤ 100; skips pruned rounds
// Only the newest 240 round logs are stored (each costs ~0.009 NEAR of storage staking).
get_stats(): { total_supply: U128, minted: U128, burned: U128, danced: U128, seeded_count: number, rounds_settled: number }

RoundLog = {
  round: number, hour: number,                     // hour = unix hour
  mm_tenths: number[],                             // 12
  minted: U128[], burned: U128[],                  // 12 each (pool mints incl. jackpot payout / evaporation)
  jackpot_paid: U128[],                            // 12
  sprouted: U128[],                                // 12, pending converted this round
  keeper: string, keeper_tip: U128,
  outlayer_request_id: string | null,   // echoed by the oracle from NEAR_REQUEST_ID (part of the attested output)
  tick_block_height: number,            // block of the tick() tx that requested this reading
  settled_ts: number
}
```

## Events (NEP-297, `EVENT_JSON:` logs)
- NEP-141 standard `nep141` v`1.0.0`: `ft_mint`, `ft_burn`, `ft_transfer`.
- Custom standard `drizzle` v`1.0.0`, event names are snake_case variants:
  - `round` data `[{ round, hour, mm_tenths, minted, burned, keeper }]`
  - `round_failed` data `[{ round, reason }]` (failed callback: tx succeeds, round not advanced, `in_flight` cleared)
  - `plant` `[{ account_id, city, amount, sprouts_after_round }]`
  - `uproot` `[{ account_id, city, shares, amount }]`
  - `rain_dance` `[{ account_id, city, amount, jackpot }]`
  - `seed` `[{ account_id, amount }]`

Stats semantics: `minted` = seeds + rain payouts + keeper tips; `burned` = evaporation + rain dances + forced-unregister burns; `danced` = rain dances only. `RoundLog.sprouted` = pending tokens converted that round; `RoundLog.keeper_tip` = tip actually paid (0 if the keeper is not registered).
