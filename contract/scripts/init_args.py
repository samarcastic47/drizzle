#!/usr/bin/env python3
"""Print the JSON arguments for Drizzle's `new(config)` initializer.

Usage:
    python3 contract/scripts/init_args.py --replay --genesis-ts 1794400000
    python3 contract/scripts/init_args.py --live --genesis-ts "$(date +%s)"

The init method takes a single `config` argument, so the printed object is
`{"config": {...}}`. Pipe it to `near ... json-args` / `cargo near deploy ...
with-init-call new json-args ...`.

`--genesis-ts` is the unix second at which round 0 starts. In replay mode the
hours are historical (first_hour = 497280 = 2026-09-24T00:00Z); in live mode
first_hour is the hour containing genesis_ts.
"""

import argparse
import json
import time

DECIMALS = 18
DEC = 10 ** DECIMALS

# Fixed order from HANDOFF.md §1 / docs/interface.md.
CITIES = [
    ("Bergen", 603900, 53200),
    ("Sao Paulo", -235500, -466300),
    ("Lagos", 65200, 33800),
    ("Singapore", 13500, 1038200),
    ("Tokyo", 356800, 1396900),
    ("Jakarta", -62000, 1068500),
    ("Mumbai", 190800, 728800),
    ("London", 515100, -1300),
    ("Lisbon", 387200, -91400),
    ("Seattle", 476100, -1223300),
    ("Cairo", 300400, 312400),
    ("Dubai", 252000, 552700),
]

# Pinned oracle WASM (docs/notes/outlayer.md §11).
ORACLE_URL = (
    "https://test.fastfs.io/drizzle-dev.testnet/outlayer.testnet/"
    "7d708ee751d708e8bfd331785204190e1741ed1ca0ce5f76dcf4317e75a7128b.wasm"
)
ORACLE_HASH = "7d708ee751d708e8bfd331785204190e1741ed1ca0ce5f76dcf4317e75a7128b"

REPLAY_FIRST_HOUR = 497280  # 2026-09-24T00:00Z
REPLAY_HOURS = 168  # 2026-09-24T00:00Z .. 2026-09-30T23:00Z, replayed in a loop


def build_config(replay: bool, genesis_ts: int) -> dict:
    if replay:
        round_seconds = 60
        first_hour = REPLAY_FIRST_HOUR
    else:
        round_seconds = 3600
        first_hour = genesis_ts // 3600

    return {
        "config": {
            "name": "Drizzle",
            "symbol": "DRIZZLE",
            "decimals": DECIMALS,
            "icon": None,
            "cities": [
                {"name": name, "lat_e4": lat, "lon_e4": lon}
                for name, lat, lon in CITIES
            ],
            "emission_per_round": str(100 * DEC),
            "evap_bps": 10,
            "rain_threshold_tenths": 2,
            "cap_tenths": 50,
            "seed_amount": str(100 * DEC),
            "keeper_tip": str(1 * DEC),
            "round_seconds": round_seconds,
            "genesis_ts": genesis_ts,
            "first_hour": first_hour,
            "replay": replay,
            "replay_hours": REPLAY_HOURS if replay else 0,
            "oracle": {
                "outlayer": "outlayer.testnet",
                "source": {
                    "WasmUrl": {
                        "url": ORACLE_URL,
                        "hash": ORACLE_HASH,
                        "build_target": "wasm32-wasip2",
                    }
                },
                "resource_limits": {
                    "max_instructions": 5_000_000_000,
                    "max_memory_mb": 64,
                    "max_execution_seconds": 20,
                },
                # 0.01 NEAR in yoctoNEAR.
                "deposit": str(10 ** 22),
            },
        }
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--replay", action="store_true", help="60 s rounds over historical hours")
    mode.add_argument("--live", action="store_true", help="1 round = 1 real hour")
    parser.add_argument(
        "--genesis-ts",
        type=int,
        default=None,
        help="unix seconds of round 0 (default: now)",
    )
    args = parser.parse_args()

    genesis_ts = args.genesis_ts if args.genesis_ts is not None else int(time.time())
    print(json.dumps(build_config(args.replay, genesis_ts), indent=2))


if __name__ == "__main__":
    main()
