#!/usr/bin/env node
/**
 * Drizzle keeper — calls `tick()` on a Drizzle contract whenever a weather round
 * is due, and reports what happened.
 *
 *   pnpm start -- --contract drizzle-replay.testnet
 *   node dist/keeper.js --contract drizzle-replay.testnet --network testnet
 *
 * Credentials come from the environment only:
 *   NEAR_ACCOUNT_ID   the keeper account (e.g. drizzle-dev.testnet)
 *   NEAR_PRIVATE_KEY  an ed25519:... secret key for that account
 *
 * The keeper never reads key files and never prints the key. `--dry-run` needs no
 * credentials at all: it only issues view calls and logs what it would do.
 */

import { createHash } from 'node:crypto';
import { setTimeout as delay } from 'node:timers/promises';
import { Account, JsonRpcProvider, KeyPair, actions, baseEncode, type KeyPairString } from 'near-api-js';
import { InvalidNonceError, TransactionTimeoutError } from 'near-api-js/rpc-errors';

import { decide, type ContractConfig, type RoundState } from './decide.js';
import { collectLogs, mergeEvents, parseEvents, txFailure, type ParsedEvents } from './events.js';
import { formatNear, isoHour, isoSecond } from './format.js';
import { configSchema, roundLogSchema, roundStateSchema } from './schema.js';
import { oneLine, safeJson } from './text.js';

/* ------------------------------------------------------------------ constants */

const TICK_GAS = 300n * 10n ** 12n; // 300 Tgas for tick() + request_execution + callback
const SETTLE_TIMEOUT_MS = 120_000; // how long to wait for a round to settle
const SETTLE_POLL_MS = 3_000;
const BACKOFF_START_MS = 10_000;
const BACKOFF_MAX_MS = 300_000;
const WARN_BALANCE = 5n * 10n ** 23n; // 0.5 NEAR
const STOP_BALANCE = 1n * 10n ** 23n; // 0.1 NEAR
const DEFAULT_INTERVAL_SEC = 10;

const RPC_URLS: Record<string, string> = {
  testnet: 'https://test.rpc.fastnear.com',
  mainnet: 'https://free.rpc.fastnear.com',
};

const USAGE = `drizzle keeper — tick the Drizzle contract whenever a round is due

Usage:
  pnpm start -- --contract <account> [options]
  node dist/keeper.js --contract <account> [options]

Options:
  --contract <id>     Drizzle contract to tick (required)
  --network <name>    testnet | mainnet            (default: testnet)
  --rpc <url>         custom NEAR RPC endpoint     (default: the network's public RPC)
  --interval <sec>    seconds between polls        (default: ${DEFAULT_INTERVAL_SEC})
  --dry-run           views only: log what would happen, never sign
  --verbose           also log every poll, not just ticks and errors
  -h, --help          show this help

Environment (required unless --dry-run):
  NEAR_ACCOUNT_ID     keeper account id
  NEAR_PRIVATE_KEY    ed25519:... secret key for NEAR_ACCOUNT_ID`;

/* --------------------------------------------------------------------- output */

let verbose = false;

function stamp(): string {
  return new Date().toISOString().replace(/\.\d{3}Z$/, 'Z');
}

function log(message: string): void {
  console.log(`${stamp()} ${message}`);
}

function warn(message: string): void {
  console.error(`${stamp()} ! ${message}`);
}

function debug(message: string): void {
  if (verbose) log(message);
}


function describeError(error: unknown): string {
  if (error instanceof Error) {
    const extra = 'context' in error ? error.context : error.cause;
    const detail = extra === undefined || extra === null ? '' : ` [${safeJson(extra)}]`;
    return oneLine(`${error.name}: ${error.message}${detail}`);
  }
  return oneLine(typeof error === 'string' ? error : safeJson(error));
}

/** Parse untrusted RPC output with a schema, prefixing failures with the call site. */
function parseOrThrow<T>(schema: { parse(value: unknown): T }, raw: unknown, label: string): T {
  try {
    return schema.parse(raw);
  } catch (error) {
    throw new Error(`${label}: ${describeError(error)}`);
  }
}

function nowSec(): number {
  return Math.floor(Date.now() / 1000);
}

/* ------------------------------------------------------------------------ CLI */

interface Options {
  contract: string;
  network: string;
  rpc: string;
  intervalSec: number;
  dryRun: boolean;
}

class UsageError extends Error {}

function parseArgs(argv: string[]): Options {
  let contract = '';
  let network = 'testnet';
  let rpc = '';
  let intervalSec = DEFAULT_INTERVAL_SEC;
  let dryRun = false;

  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    const equalsAt = arg.startsWith('--') ? arg.indexOf('=') : -1;
    const flag = equalsAt > 0 ? arg.slice(0, equalsAt) : arg;
    const inline = equalsAt > 0 ? arg.slice(equalsAt + 1) : undefined;

    const value = (): string => {
      const next = inline ?? argv[++i];
      if (next === undefined || next.startsWith('--')) throw new UsageError(`${flag} needs a value`);
      return next;
    };

    switch (flag) {
      case '--':
        // pnpm forwards the separator itself: `pnpm start -- --flag`.
        break;
      case '-h':
      case '--help':
        console.log(USAGE);
        process.exit(0);
        break;
      case '--contract':
        contract = value();
        break;
      case '--network':
        network = value();
        break;
      case '--rpc':
        rpc = value();
        break;
      case '--interval': {
        const raw = value();
        const parsed = Number(raw);
        if (!Number.isFinite(parsed) || parsed < 1) {
          throw new UsageError(`--interval must be a number of seconds >= 1 (got "${raw}")`);
        }
        intervalSec = parsed;
        break;
      }
      case '--dry-run':
        dryRun = true;
        break;
      case '--verbose':
        verbose = true;
        break;
      default:
        throw new UsageError(`unknown argument "${arg}"`);
    }
  }

  if (!contract) throw new UsageError('--contract <account id> is required');
  if (!rpc) {
    rpc = RPC_URLS[network] ?? '';
    if (!rpc) throw new UsageError(`unknown network "${network}" — pass --rpc <url>`);
  }

  return { contract, network, rpc, intervalSec, dryRun };
}

/* --------------------------------------------------------------- credentials */

interface Credentials {
  accountId: string;
  privateKey: KeyPairString;
}

function requireCredentials(): Credentials {
  const accountId = (process.env.NEAR_ACCOUNT_ID ?? '').trim();
  const privateKey = (process.env.NEAR_PRIVATE_KEY ?? '').trim();

  const missing = [!accountId && 'NEAR_ACCOUNT_ID', !privateKey && 'NEAR_PRIVATE_KEY'].filter(Boolean);
  if (missing.length > 0) {
    console.error(
      [
        `drizzle keeper: missing credentials — set ${missing.join(' and ')} in the environment.`,
        '  export NEAR_ACCOUNT_ID=drizzle-dev.testnet',
        "  export NEAR_PRIVATE_KEY=ed25519:...   # the account's secret key, never committed",
        'Keys are only read from the environment; the keeper never opens key files.',
        'Use --dry-run to watch the contract without any credentials.',
      ].join('\n'),
    );
    process.exit(1);
  }

  if (!privateKey.startsWith('ed25519:')) {
    console.error('drizzle keeper: NEAR_PRIVATE_KEY must be an ed25519:... secret key for NEAR_ACCOUNT_ID.');
    process.exit(1);
  }

  // The prefix was validated above; the library types want the `ed25519:${string}`
  // template, which a plain string does not narrow to.
  try {
    KeyPair.fromString(privateKey as KeyPairString);
  } catch {
    // Deliberately does not echo the key back.
    console.error('drizzle keeper: NEAR_PRIVATE_KEY is not a valid ed25519 secret key (expected ed25519:<base58>).');
    process.exit(1);
  }

  // The prefix and base58 shape were just validated; the compiler cannot narrow a
  // plain string to the library's `ed25519:${string}` template type.
  return { accountId, privateKey: privateKey as KeyPairString };
}

/* ---------------------------------------------------------------- chain calls */

type Provider = JsonRpcProvider;

/** View call returning raw JSON; callers parse it with a schema. */
async function viewCall(
  provider: Provider,
  contract: string,
  method: string,
  args: Record<string, unknown> = {},
): Promise<unknown> {
  const result = await provider.callFunction({
    contractId: contract,
    method,
    args,
    blockQuery: { finality: 'final' },
  });
  if (result === undefined || result === null) {
    throw new Error(`${contract}.${method}() returned no value — is it deployed and initialised?`);
  }
  return result;
}

async function nearBalance(provider: Provider, accountId: string): Promise<bigint> {
  const account = await provider.viewAccount({ accountId, blockQuery: { finality: 'final' } });
  return account.amount;
}

async function readRoundState(provider: Provider, contract: string): Promise<RoundState> {
  return parseOrThrow(roundStateSchema, await viewCall(provider, contract, 'get_round_state'), 'get_round_state()');
}

async function readConfig(provider: Provider, contract: string): Promise<ContractConfig> {
  return parseOrThrow(configSchema, await viewCall(provider, contract, 'get_config'), 'get_config()');
}

function oracleDeposit(config: ContractConfig): bigint {
  const raw = config.oracle.deposit;
  let deposit: bigint;
  try {
    deposit = BigInt(String(raw));
  } catch {
    throw new Error(`get_config().oracle.deposit is not a yoctoNEAR integer: ${safeJson(raw, 80)}`);
  }
  if (deposit <= 0n) throw new Error(`get_config().oracle.deposit must be positive, got ${deposit}`);
  return deposit;
}

/* ------------------------------------------------------------------ tick flow */

type TickResult = 'settled' | 'failed' | 'timeout';

interface TickReport {
  round: number;
  hour: number;
  result: TickResult;
  requestId: string | null;
  txHash: string;
  error?: string;
  spentYocto: bigint;
}

let stopping = false;
const aborter = new AbortController();

/** Sleep for `ms`; returns false if a signal stopped the keeper meanwhile. */
async function pause(ms: number): Promise<boolean> {
  if (stopping) return false;
  try {
    await delay(ms, undefined, { signal: aborter.signal });
    return true;
  } catch {
    return false;
  }
}

interface Settlement {
  result: TickResult;
  error?: string;
  events: ParsedEvents;
}

/**
 * Sign `tick()` locally, so the tx hash is known even when the RPC gives up.
 *
 * Waiting for the *whole* transaction is a trap: OutLayer's yield only resolves
 * when the TEE worker resumes it (10–60 s), so `send_tx` with `wait_until: FINAL`
 * times out with `Timeout error [408]` even though the tick landed. We only wait
 * for the first receipt (`EXECUTED_OPTIMISTIC`) and treat a 408 as "submitted,
 * outcome unknown" — the round state poll decides what actually happened.
 */
async function submitTick(
  account: Account,
  provider: Provider,
  contract: string,
  deposit: bigint,
): Promise<{ txHash: string; outcome: Awaited<ReturnType<Provider['sendTransactionUntil']>> | null }> {
  const signedTx = await account.createSignedTransaction({
    receiverId: contract,
    actions: [actions.functionCall('tick', {}, TICK_GAS, deposit)],
  });
  // The hash is sha256 of the borsh-encoded signed transaction, base58.
  const txHash = baseEncode(createHash('sha256').update(signedTx.encode()).digest());

  try {
    const outcome = await provider.sendTransactionUntil(signedTx, 'EXECUTED_OPTIMISTIC');
    return { txHash, outcome };
  } catch (error) {
    // The RPC reports the yield-receipt wait as a generic `Timeout error [408]`,
    // not always as TransactionTimeoutError: both mean "submitted, unknown".
    if (error instanceof TransactionTimeoutError || isRpcTimeout(error)) return { txHash, outcome: null };
    throw error;
  }
}

function isRpcTimeout(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error);
  return /timeout/i.test(message) || /\[408\]/.test(message);
}

/** Logs of the tick transaction, once the node knows them; null while unknown. */
async function fetchTxEvents(provider: Provider, txHash: string, accountId: string): Promise<ParsedEvents | null> {
  try {
    const status = await provider.viewTransactionStatusWithReceipts({ txHash, accountId, waitUntil: 'NONE' });
    return parseEvents(collectLogs(status));
  } catch {
    return null; // not submitted to this node yet, or still in the mempool
  }
}

/**
 * Poll the contract until the round settles (or its in-flight lock clears) and
 * collect the drizzle events, which only appear once the OutLayer callback has
 * run. Total budget: 120 s. After settlement we give the tx-status logs a short
 * grace period (5 polls) and then stop.
 */
async function waitForTickOutcome(
  provider: Provider,
  contract: string,
  accountId: string,
  txHash: string,
  round: number,
  initialEvents: ParsedEvents,
): Promise<Settlement> {
  const deadline = Date.now() + SETTLE_TIMEOUT_MS;
  let events = initialEvents;
  let result: TickResult | null = null;
  let error: string | undefined;
  let lastError: string | undefined;
  let logDeadline = Number.POSITIVE_INFINITY;

  while (Date.now() < deadline) {
    if (!(await pause(SETTLE_POLL_MS))) {
      return {
        result: result ?? 'timeout',
        error: error ?? 'keeper stopped while the round was in flight',
        events,
      };
    }

    if (result === null) {
      try {
        const state = await readRoundState(provider, contract);
        if (state.next_round > round) {
          result = 'settled';
          logDeadline = Date.now() + 5 * SETTLE_POLL_MS;
        } else if (!state.in_flight || state.in_flight.round !== round) {
          result = 'failed';
          error = `in_flight cleared but round ${round} did not settle (the oracle call failed)`;
          logDeadline = Date.now() + 5 * SETTLE_POLL_MS;
        }
      } catch (pollError) {
        lastError = describeError(pollError);
      }
    }

    if (events.rounds.length === 0 && events.failures.length === 0 && Date.now() <= logDeadline) {
      const fromTx = await fetchTxEvents(provider, txHash, accountId);
      if (fromTx) events = mergeEvents(events, fromTx);
    }

    const haveRoundEvent = events.rounds.length > 0 || events.failures.length > 0;
    if (result !== null && (haveRoundEvent || Date.now() > logDeadline)) break;
  }

  if (result === null) {
    return {
      result: 'timeout',
      error: lastError
        ? `settlement poll failed: ${lastError}`
        : `no settlement after ${SETTLE_TIMEOUT_MS / 1000}s`,
      events,
    };
  }
  return { result, error, events };
}

async function lookupRequestId(provider: Provider, contract: string, round: number): Promise<string | null> {
  try {
    const raw = await viewCall(provider, contract, 'get_round_log', { round });
    return roundLogSchema.parse(raw).outlayer_request_id;
  } catch {
    return null;
  }
}

async function runTick(
  account: Account,
  provider: Provider,
  contract: string,
  config: ContractConfig,
  state: RoundState,
): Promise<TickReport> {
  const deposit = oracleDeposit(config);
  const round = state.next_round;
  const balanceBefore = await nearBalance(provider, account.accountId);

  const { txHash, outcome } = await submitTick(account, provider, contract, deposit);
  const sentEvents = parseEvents(outcome ? collectLogs(outcome) : []);
  const rejection = outcome ? txFailure(outcome.status) : undefined;

  let settlement: Settlement;
  if (rejection !== undefined) {
    // The transaction itself was rejected (e.g. "round not due"): nothing to poll.
    settlement = { result: 'failed', error: rejection, events: sentEvents };
  } else {
    if (!outcome) warn(`tick ${txHash} not confirmed by the RPC in time — polling the round state`);
    settlement = await waitForTickOutcome(provider, contract, account.accountId, txHash, round, sentEvents);
  }

  const { result, events } = settlement;
  let error = settlement.error;
  if (result === 'failed' && rejection === undefined && events.failures.length > 0) {
    error = events.failures[events.failures.length - 1].reason;
  }

  if (events.rounds.length > 0) {
    const settled = events.rounds[events.rounds.length - 1];
    debug(`round event: round=${settled.round} mm_tenths=[${(settled.mm_tenths ?? []).join(',')}]`);
  }

  const requestId =
    events.requestId ?? (result === 'settled' ? await lookupRequestId(provider, contract, round) : null);
  const balanceAfter = await nearBalance(provider, account.accountId);

  return { round, hour: state.next_hour, result, requestId, txHash, error, spentYocto: balanceBefore - balanceAfter };
}

/* ----------------------------------------------------------------------- main */

async function main(): Promise<number> {
  let options: Options;
  try {
    options = parseArgs(process.argv.slice(2));
  } catch (error) {
    if (error instanceof UsageError) {
      console.error(`drizzle keeper: ${error.message}\n\n${USAGE}`);
      return 1;
    }
    throw error;
  }

  const credentials = options.dryRun ? null : requireCredentials();
  const provider = new JsonRpcProvider({ url: options.rpc }, { retries: 1, wait: 750, backoff: 2 });
  // Rebuilt if a cached access-key nonce goes stale (another keeper sharing the key).
  let account = credentials ? new Account(credentials.accountId, provider, credentials.privateKey) : null;

  // Process managers often escalate: `timeout 40` sends SIGINT *and* SIGTERM, and
  // a terminal Ctrl-C can be followed by SIGTERM. Treat any signal during shutdown
  // as part of the same request; only a repeated *identical* signal forces a quit.
  let stopRequestedBy: string | null = null;
  const onSignal = (signal: string) => {
    if (stopRequestedBy === null) {
      stopRequestedBy = signal;
      stopping = true;
      warn(`${signal} received — finishing the current step and exiting (repeat to force quit)`);
      aborter.abort();
      return;
    }
    if (signal === stopRequestedBy) {
      warn(`${signal} again — exiting immediately`);
      process.exit(130);
    }
    debug(`${signal} received while already stopping — ignored`);
  };
  process.on('SIGINT', () => onSignal('SIGINT'));
  process.on('SIGTERM', () => onSignal('SIGTERM'));

  const mode = credentials ? `signing as ${credentials.accountId}` : 'dry-run (views only)';
  log(
    `drizzle keeper starting — contract=${options.contract} network=${options.network} rpc=${options.rpc} ` +
      `interval=${options.intervalSec}s mode=${mode}`,
  );

  let config: ContractConfig | null = null;
  let backoffMs = BACKOFF_START_MS;
  let totalSpent = 0n;
  const tickTimes: number[] = [];

  while (!stopping) {
    try {
      if (!config) {
        const loaded = await readConfig(provider, options.contract);
        const deposit = oracleDeposit(loaded);
        config = loaded;
        log(
          `config loaded — round_seconds=${config.round_seconds} replay=${String(config.replay ?? 'unknown')} ` +
            `oracle=${config.oracle.outlayer} deposit=${formatNear(deposit)} NEAR`,
        );
      }

      const state = await readRoundState(provider, options.contract);

      if (account) {
        const balance = await nearBalance(provider, account.accountId);
        if (balance < STOP_BALANCE) {
          warn(
            `balance ${formatNear(balance)} NEAR is below 0.1 NEAR — not ticking. ` +
              `Top up ${account.accountId} to keep the keeper running.`,
          );
          backoffMs = BACKOFF_START_MS;
          if (!(await pause(options.intervalSec * 1000))) break;
          continue;
        }
        if (balance < WARN_BALANCE) {
          warn(`balance ${formatNear(balance)} NEAR is below 0.5 NEAR — top up ${account.accountId} soon.`);
        }
      }

      const decision = decide(state, config, nowSec());

      if (decision === 'wait') {
        const dueIn = state.due_at - Math.max(nowSec(), state.now);
        debug(
          `round ${state.next_round} (${isoHour(state.next_hour)}) ` +
            (dueIn > 0
              ? `due in ${dueIn}s`
              : `in flight since ${isoSecond(state.in_flight?.started_ts ?? state.now)}`),
        );
        backoffMs = BACKOFF_START_MS;
        if (!(await pause(options.intervalSec * 1000))) break;
        continue;
      }

      if (decision === 'retry-expired') {
        warn(
          `round ${state.next_round} still in flight since ${isoSecond(state.in_flight?.started_ts ?? state.now)} ` +
            `(> 600s) — retrying tick()`,
        );
      }

      if (options.dryRun) {
        log(
          `would tick round=${state.next_round} hour=${isoHour(state.next_hour)} ` +
            `deposit=${formatNear(oracleDeposit(config))} NEAR gas=300 Tgas ` +
            `in_flight=${state.in_flight ? `round ${state.in_flight.round}` : 'none'}`,
        );
        backoffMs = BACKOFF_START_MS;
        if (!(await pause(options.intervalSec * 1000))) break;
        continue;
      }

      if (!account) throw new Error('internal error: dry-run should have returned before signing');
      let report: TickReport;
      try {
        report = await runTick(account, provider, options.contract, config, state);
      } catch (tickError) {
        if (!(tickError instanceof InvalidNonceError) || !credentials) throw tickError;
        // near-api-js caches nonces per key; a fresh Account re-reads them from chain.
        warn('access-key nonce was stale — rebuilding the signer and retrying once');
        account = new Account(credentials.accountId, provider, credentials.privateKey);
        if (!(await pause(2_000))) break;
        report = await runTick(account, provider, options.contract, config, state);
      }
      totalSpent += report.spentYocto;

      const tickedAt = Date.now();
      tickTimes.push(tickedAt);
      while (tickTimes.length > 0 && tickedAt - tickTimes[0] > 3_600_000) tickTimes.shift();

      const parts = [
        `round=${report.round}`,
        `hour=${isoHour(report.hour)}`,
        `result=${report.result}`,
        `request_id=${report.requestId ?? '-'}`,
        `tx=${report.txHash}`,
        `spent=${formatNear(report.spentYocto)} NEAR`,
        `total=${formatNear(totalSpent)} NEAR`,
        `ticks/hour=${tickTimes.length}`,
      ];
      if (report.error) parts.push(`error="${oneLine(report.error, 200)}"`);

      if (report.result === 'settled') {
        log(parts.join(' '));
        backoffMs = BACKOFF_START_MS;
        // Catch-up: no sleep here. The next iteration re-reads the state and ticks
        // again immediately if another round is already due.
        continue;
      }

      warn(parts.join(' '));

      // A failed or timed-out tick is a transaction failure: back off before the
      // next attempt instead of hammering the contract (10 s → 5 min).
      const waitMs = backoffMs;
      backoffMs = Math.min(backoffMs * 2, BACKOFF_MAX_MS);
      debug(`waiting ${Math.round(waitMs / 1000)}s before the next attempt`);
      if (!(await pause(waitMs))) break;
    } catch (error) {
      const waitMs = backoffMs;
      backoffMs = Math.min(backoffMs * 2, BACKOFF_MAX_MS);
      warn(`${describeError(error)} — backing off ${Math.round(waitMs / 1000)}s`);
      if (!(await pause(waitMs))) break;
    }
  }

  log(`stopped — ${tickTimes.length} tick(s) in the last hour, ${formatNear(totalSpent)} NEAR spent this run`);
  return 0;
}

main()
  .then((code) => {
    // The RPC transport keeps sockets in an undici keep-alive pool, so the event
    // loop would stay alive after a graceful stop; exit explicitly. stdout writes
    // to pipes/TTYs are synchronous on POSIX, so the final lines are flushed.
    process.exit(code);
  })
  .catch((error) => {
    console.error(`drizzle keeper: fatal: ${describeError(error)}`);
    process.exit(1);
  });
