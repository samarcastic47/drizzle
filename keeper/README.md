# Drizzle keeper

A small Node/TypeScript CLI that keeps a [Drizzle](../README.md) contract ticking.
Every round Drizzle needs someone to call `tick()`; the contract then asks
[OutLayer](https://app.outlayer.ai) to run the pinned weather oracle, and settles
the round in the callback. The keeper is that "someone" — anyone can run one, and
the best keeper earns the round's keeper tip.

What it does, per poll:

1. reads `get_round_state()` (and `get_config()` once, then caches it);
2. calls `tick()` with the oracle deposit and 300 Tgas when the round is due
   (`now >= due_at` and nothing is in flight, or the in-flight request is older
   than 10 minutes);
3. waits only for the transaction's first receipt (`EXECUTED_OPTIMISTIC`) and then
   polls `get_round_state()` until the round settles or `in_flight` clears
   (max 120 s). Waiting for `FINAL` would nearly always time out: the tick's
   OutLayer yield only resolves when the TEE worker resumes it, 10–60 s later;
4. reads the transaction status (`EXPERIMENTAL_tx_status`, `wait_until: NONE`) until
   the `drizzle` `round` / `round_failed` events and OutLayer's `request_id` show up
   in the receipt logs;
5. prints one line per tick with the round, the hour, the result, the OutLayer
   `request_id`, the tx hash, what the keeper spent, the running total and the
   tick rate.

When several rounds are behind (the keeper was offline, or the replay contract
runs at 1 round/minute), it ticks them back-to-back without sleeping the interval.

## Requirements

- Node.js ≥ 22 (developed on Node 26)
- `pnpm` (or `npm`; the lockfile is pnpm's)

## Build

```bash
cd keeper
pnpm install
pnpm build          # tsc -> dist/
```

## Run

```bash
# with pnpm (arguments after -- go to the keeper)
pnpm start -- --contract drizzle-replay.testnet

# or directly
node dist/keeper.js --contract drizzle-replay.testnet --network testnet --interval 10

# watch without credentials: views only, logs what it would do
node dist/keeper.js --contract drizzle-replay.testnet --dry-run --verbose
```

Flags:

| Flag | Default | Meaning |
|---|---|---|
| `--contract <id>` | — (required) | Drizzle contract to tick |
| `--network <name>` | `testnet` | `testnet` or `mainnet`; picks the public RPC |
| `--rpc <url>` | network default | Custom NEAR RPC endpoint |
| `--interval <sec>` | `10` | Seconds between polls when nothing is due |
| `--dry-run` | off | Views only: log what it *would* do, never sign |
| `--verbose` | off | Also log every poll, not just ticks and errors |
| `-h`, `--help` | — | Usage |

## Credentials

Only from the environment — the keeper never reads key files and never logs the key:

```bash
export NEAR_ACCOUNT_ID=drizzle-dev.testnet
export NEAR_PRIVATE_KEY=ed25519:...
```

`.env.example` is a template; the keeper does not load it by itself. Either export
the variables in your shell, or use a process manager's env-file support:

```bash
set -a; . ./.env; set +a          # shell
systemd: EnvironmentFile=/path/to/.env
pm2:     pm2 start dist/keeper.js --env-file .env -- --contract drizzle-replay.testnet
```

Without `NEAR_ACCOUNT_ID` / `NEAR_PRIVATE_KEY` the keeper exits with status 1 and a
message telling you what is missing. `--dry-run` needs no credentials at all.

## Example output

```
2026-11-11T12:00:05Z drizzle keeper starting — contract=drizzle-replay.testnet network=testnet rpc=https://rpc.testnet.near.org interval=10s mode=signing as drizzle-dev.testnet
2026-11-11T12:00:06Z config loaded — round_seconds=60 replay=true oracle=outlayer.testnet deposit=0.004500 NEAR
2026-11-11T12:00:16Z round=12 hour=2026-09-24T12:00Z result=settled request_id=22023 tx=7T2fBww6cqcMJQw7RNoevLT7oVCZPmJSVKLPtjYyFaoY spent=0.001910 NEAR total=0.001910 NEAR ticks/hour=1
```

`result` is one of `settled` (the round advanced), `failed` (the tx or the oracle
call failed — the contract's message is appended as `error="round not due"`), or
`timeout` (nothing settled within 120 s).

## Cost

A tick costs the keeper the OutLayer deposit plus gas. On testnet the measured
total for one real round was **0.0019 NEAR** (OutLayer charged 0.0011776 for the
enclave run and refunded the rest; gas was ~8 Tgas) — budget **0.002–0.005 NEAR
per round**. The exact figure for each tick is printed as `spent=… NEAR`, measured
from the keeper's balance before and after the round settles.

The replay contract runs 1 round/minute ≈ 1,440 ticks/day; budget accordingly.

## Balance guards

Each poll checks the keeper's balance:

- below **0.5 NEAR**: warns on every loop;
- below **0.1 NEAR**: stops ticking (keeps warning) so it does not spam failing
  transactions. Top up and it resumes by itself.

## Errors, backoff, shutdown

- `Timeout error [408]` is **not** a failure. It means the RPC gave up waiting for
  OutLayer's yield to resume, while the tick itself usually landed. The keeper
  signs the transaction locally (so it always knows the tx hash), treats the 408 as
  "submitted, outcome unknown", and resolves it by polling `get_round_state()`. Only
  a rejected transaction or a round that never advances is reported as `failed`.
- Any other RPC or transaction error is logged and retried with exponential
  backoff: 10 s → 20 s → 40 s … capped at 5 minutes, reset on the next successful
  step. A single error never kills the keeper, and `--dry-run` against an
  undeployed contract just keeps backing off with the RPC's message.
- `Ctrl-C` (`SIGINT`) or `SIGTERM` stops after the current step and exits 0, so
  `systemd`/`pm2` restarts stay clean. A supervisor that sends a second, different
  signal while shutting down is ignored; sending the same signal twice forces an
  immediate exit (130). Note that `timeout` itself reports `124` whenever it
  fires, even when the program exits cleanly — send `kill -INT <pid>` yourself to
  observe exit status 0.

## Run unattended

```bash
# nohup
nohup node dist/keeper.js --contract drizzle-replay.testnet >> keeper.log 2>&1 &

# pm2
pm2 start dist/keeper.js --name drizzle-keeper -- --contract drizzle-replay.testnet
pm2 save
```

systemd user unit (`~/.config/systemd/user/drizzle-keeper.service`):

```ini
[Unit]
Description=Drizzle keeper
After=network-online.target

[Service]
Type=simple
WorkingDirectory=%h/drizzle/keeper
EnvironmentFile=%h/drizzle/keeper/.env
ExecStart=/usr/bin/node %h/drizzle/keeper/dist/keeper.js --contract drizzle-replay.testnet --interval 10
Restart=always
RestartSec=15
StandardOutput=append:%h/drizzle/keeper/keeper.log
StandardError=append:%h/drizzle/keeper/keeper.log

[Install]
WantedBy=default.target
```

```bash
systemctl --user daemon-reload
systemctl --user enable --now drizzle-keeper
loginctl enable-linger "$USER"     # keep it running while logged out
journalctl --user -u drizzle-keeper -f
```

## Tests

`decide(state, config, nowSec)` — the pure "should we tick?" rule — is unit tested:

```bash
pnpm test        # builds, then node --test dist/*.test.js
```
