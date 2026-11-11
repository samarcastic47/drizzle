# drizzle-oracle

WASI Preview 2 module for the Drizzle ($DRIZZLE) hackathon project. It is run
by [OutLayer](https://outlayer.ai) inside a TEE by a NEAR contract.

The module reads a round input from stdin, makes **one** batched Open-Meteo
forecast request for all requested cities, and prints the hourly precipitation
readings as JSON.

## Notice

Forked from [out-layer/weather-example](https://github.com/out-layer/weather-example)
(MIT OR Apache-2.0). Upstream copyright notices are preserved in
`LICENSE-MIT` and `LICENSE-APACHE`.

## Interface

stdin:

```json
{"hour": 497294, "cities": [[603900, 53200], [13500, 1038200]]}
```

* `hour`: unix hour (u64), interpreted as UTC.
* `cities`: 1 to 50 `[lat_e4, lon_e4]` pairs (integer degrees × 10⁴). The
  contract sends exactly 12.

stdout on success (one line):

```json
{"hour":497294,"mm_tenths":[0,0],"source":"open-meteo","outlayer_request_id":null}
```

`mm_tenths` has one entry per input city, in input order, and is
`round(mm * 10)` clamped to `u16::MAX`. `outlayer_request_id` echoes the
decimal `NEAR_REQUEST_ID` env var injected by the OutLayer worker as a JSON
string, or is `null` when it is absent, empty, or not a valid u64 (e.g. under
local wasmtime).

On any error the module prints `{"error":"..."}` on stdout and exits non-zero.
It never prints partial data.

## Request

```
https://api.open-meteo.com/v1/forecast?latitude=<csv>&longitude=<csv>&hourly=precipitation&start_hour=<H>&end_hour=<H>&timezone=UTC
```

`<H>` is `YYYY-MM-DDTHH:00` (UTC) and the coordinate CSV uses the same decimal
formatting as JavaScript's `(x / 1e4).toString()`.

## Build

```bash
./build.sh          # produces drizzle-oracle.wasm
cargo test          # host-side pure-logic tests
./verify.sh         # compare the wasm against a direct curl + python3 request
```

Run locally:

```bash
~/.wasmtime/bin/wasmtime run -S http ./drizzle-oracle.wasm \
  < fixtures/input_2026-09-24T14.json
```

The forecast API only covers roughly the past 92 days; older hours return
HTTP 400 (the archive API is deliberately out of scope).

## License

MIT OR Apache-2.0.
