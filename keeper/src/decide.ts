/**
 * Pure decision logic for the Drizzle keeper: given what the contract says about
 * the next round and the current time, should we call `tick()` now?
 *
 * Kept free of I/O and of near-api-js so it can be unit tested with `node --test`.
 *
 * The contract allows `tick()` when `now >= due_at` and nothing is in flight; an
 * in-flight request older than 10 minutes is considered dead (OutLayer's own
 * `EXECUTION_TIMEOUT` is 600 s and a stranded yield never fires our callback, so
 * the contract lets a keeper retry after that window — see HANDOFF §2.2).
 */

/** `get_round_state()` as returned by the contract (docs/interface.md). */
export interface RoundState {
  /** Round that the next `tick()` will settle. */
  next_round: number;
  /** Unix hour (seconds) covered by `next_round`. */
  next_hour: number;
  /** Unix seconds at which `tick()` becomes allowed. */
  due_at: number;
  /** The request currently being executed by OutLayer, if any. */
  in_flight: { round: number; started_ts: number } | null;
  /** Block timestamp of the node that answered the view, in unix seconds. */
  now: number;
}

/** Subset of `get_config()` the keeper needs. */
export interface ContractConfig {
  /** yoctoNEAR string, attached to every `tick()` for OutLayer. */
  oracle: { outlayer: string; deposit: string | number | bigint };
  /** Seconds per round (3600 live, 60 replay). Informational. */
  round_seconds: number;
  replay?: boolean;
}

/** How long an in-flight OutLayer request may stay unanswered before we retry. */
export const IN_FLIGHT_TIMEOUT_SECS = 600;

export type Decision = 'tick' | 'wait' | 'retry-expired';

/**
 * Decide what the keeper should do right now.
 *
 * `nowSec` is the keeper's wall clock (unix seconds); if the contract's own block
 * timestamp is ahead of it, the contract's clock wins, because `due_at` is
 * expressed in block time.
 *
 * - `tick`: a round is due and nothing is blocking it.
 * - `wait`: nothing to do yet (round not due, or a request is still in flight).
 * - `retry-expired`: a round is due and the in-flight request is older than
 *   10 minutes, so the contract will accept a replacement `tick()`.
 */
export function decide(state: RoundState, config: ContractConfig, nowSec: number): Decision {
  // `config` carries no timing input for this decision (the contract publishes
  // `due_at` directly); it is part of the signature because callers already have
  // it and future guards (paused / halted contracts) would read from here.
  void config;

  const chainNow = state.now === undefined || state.now === null ? 0 : num(state.now);
  const now = Math.max(Math.floor(nowSec), chainNow);

  if (now < num(state.due_at)) return 'wait';
  if (!state.in_flight) return 'tick';

  return now - num(state.in_flight.started_ts) > IN_FLIGHT_TIMEOUT_SECS ? 'retry-expired' : 'wait';
}

/** Tolerate u64 fields that arrive as numbers, strings or bigints. */
export function num(value: unknown): number {
  if (typeof value === 'number' && Number.isFinite(value)) return value;
  if (typeof value === 'bigint') {
    const asNumber = Number(value);
    if (Number.isSafeInteger(asNumber)) return asNumber;
  }
  if (typeof value === 'string' && value.trim() !== '') {
    const parsed = Number(value);
    if (Number.isFinite(parsed)) return parsed;
  }
  throw new Error(`expected a number, got ${JSON.stringify(value) ?? String(value)}`);
}
