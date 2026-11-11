#!/usr/bin/env bash
# Deploy the Drizzle contract and call `new(config)`.
#
#   ./scripts/deploy.sh <account.testnet> <replay|live> [genesis_ts]
#
# genesis_ts defaults to now. Run from contract/ (or anywhere; the script cd's).
# The signing account must be the contract account itself (or hold its full
# access key): `new` is #[private].
set -euo pipefail

cd "$(dirname "$0")/.."

ACCOUNT="${1:?usage: deploy.sh <account.testnet> <replay|live> [genesis_ts]}"
MODE="${2:?usage: deploy.sh <account.testnet> <replay|live> [genesis_ts]}"
GENESIS_TS="${3:-$(date +%s)}"

case "$MODE" in
  replay|live) ;;
  *) echo "mode must be 'replay' or 'live' (got '$MODE')" >&2; exit 2 ;;
esac

ARGS="$(python3 scripts/init_args.py "--$MODE" --genesis-ts "$GENESIS_TS")"

echo "Deploying to $ACCOUNT ($MODE, genesis_ts=$GENESIS_TS)" >&2
exec cargo near deploy build-non-reproducible-wasm "$ACCOUNT" \
  with-init-call new json-args "$ARGS" \
  prepaid-gas '100 Tgas' attached-deposit '0 NEAR' \
  network-config testnet sign-with-keychain send
