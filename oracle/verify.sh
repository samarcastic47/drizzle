#!/bin/bash
# Build the same Open-Meteo URL independently with python3 (stdlib), fetch it
# with curl, compute the 12 tenths, run the wasm, and diff the two mm_tenths
# arrays. Prints MATCH or exits 1.
set -euo pipefail
cd "$(dirname "$0")"

INPUT="fixtures/input_2026-09-24T14.json"
WASM="./drizzle-oracle.wasm"
WASMTIME="${WASMTIME:-$HOME/.wasmtime/bin/wasmtime}"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT
RESPONSE="$TMP_DIR/openmeteo.json"

URL=$(python3 - "$INPUT" <<'PY'
import datetime
import json
import sys

with open(sys.argv[1], encoding="utf-8") as f:
    inp = json.load(f)

hour_str = datetime.datetime.fromtimestamp(
    inp["hour"] * 3600, datetime.timezone.utc
).strftime("%Y-%m-%dT%H:00")


def coord(e4):
    sign = "-" if e4 < 0 else ""
    int_part, frac = divmod(abs(e4), 10_000)
    if frac == 0:
        return f"{sign}{int_part}"
    return f"{sign}{int_part}." + str(frac).zfill(4).rstrip("0")


cities = inp["cities"]
latitude = ",".join(coord(c[0]) for c in cities)
longitude = ",".join(coord(c[1]) for c in cities)
print(
    "https://api.open-meteo.com/v1/forecast"
    f"?latitude={latitude}&longitude={longitude}"
    f"&hourly=precipitation&start_hour={hour_str}&end_hour={hour_str}&timezone=UTC"
)
PY
)
echo "verify: GET $URL" >&2
curl -sS --fail --max-time 60 "$URL" -o "$RESPONSE"

EXPECTED=$(python3 - "$INPUT" "$RESPONSE" <<'PY'
import datetime
import json
import sys

with open(sys.argv[1], encoding="utf-8") as f:
    inp = json.load(f)
with open(sys.argv[2], encoding="utf-8") as f:
    data = json.load(f)

hour_str = datetime.datetime.fromtimestamp(
    inp["hour"] * 3600, datetime.timezone.utc
).strftime("%Y-%m-%dT%H:00")

if isinstance(data, dict):
    data = [data]

tenths = []
for element in data:
    index = element["hourly"]["time"].index(hour_str)
    mm = element["hourly"]["precipitation"][index]
    if mm is None:
        mm = 0
    # Rust's f64::round is half-away-from-zero; precipitation is >= 0.
    tenths.append(min(int(mm * 10 + 0.5), 65_535))

print(json.dumps(tenths))
PY
)

ACTUAL=$("$WASMTIME" run -S http "$WASM" < "$INPUT")
ACTUAL_TENTHS=$(python3 -c \
  'import json,sys; print(json.dumps(json.loads(sys.argv[1])["mm_tenths"]))' "$ACTUAL")

echo "expected: $EXPECTED"
echo "actual:   $ACTUAL_TENTHS"

if [ "$EXPECTED" = "$ACTUAL_TENTHS" ]; then
    echo MATCH
else
    echo MISMATCH
    exit 1
fi
