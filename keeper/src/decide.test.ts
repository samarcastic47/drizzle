import test from 'node:test';
import assert from 'node:assert/strict';

import { decide, IN_FLIGHT_TIMEOUT_SECS, num, type ContractConfig, type RoundState } from './decide.js';

const config: ContractConfig = {
  oracle: { outlayer: 'outlayer.testnet', deposit: '4500000000000000000000' },
  round_seconds: 60,
  replay: true,
};

const now = 1_800_000_000;
const dueAt = now - 5;

function state(overrides: Partial<RoundState> = {}): RoundState {
  return { next_round: 7, next_hour: 497_280 + 7 * 3600, due_at: dueAt, in_flight: null, now, ...overrides };
}

test('waits while the round is not due yet', () => {
  assert.equal(decide(state({ due_at: now + 1 }), config, now), 'wait');
});

test('ticks a due round when nothing is in flight', () => {
  assert.equal(decide(state(), config, now), 'tick');
});

test('ticks exactly at due_at', () => {
  assert.equal(decide(state({ due_at: now }), config, now), 'tick');
});

test('waits while a fresh request is in flight', () => {
  const inFlight = { round: 7, started_ts: now - 30 };
  assert.equal(decide(state({ in_flight: inFlight }), config, now), 'wait');
});

test('waits on the 600 s boundary (only strictly older counts as expired)', () => {
  const inFlight = { round: 7, started_ts: now - IN_FLIGHT_TIMEOUT_SECS };
  assert.equal(decide(state({ in_flight: inFlight }), config, now), 'wait');
});

test('retries an in-flight request older than 600 s', () => {
  const inFlight = { round: 7, started_ts: now - IN_FLIGHT_TIMEOUT_SECS - 1 };
  assert.equal(decide(state({ in_flight: inFlight }), config, now), 'retry-expired');
});

test("an in-flight request never blocks a round that is not due", () => {
  const inFlight = { round: 7, started_ts: now - 10_000 };
  assert.equal(decide(state({ due_at: now + 60, in_flight: inFlight }), config, now), 'wait');
});

test('uses the contract clock when it is ahead of the wall clock', () => {
  // Keeper clock is 10 s behind the block time: due_at has already passed on chain.
  assert.equal(decide(state({ due_at: now - 1, now: now + 10 }), config, now - 10), 'tick');
});

test('accepts u64 fields serialised as strings or bigints', () => {
  const s = state({ due_at: String(now - 1) as unknown as number, now: BigInt(now) as unknown as number });
  assert.equal(decide(s, config, now), 'tick');
  assert.equal(num('42'), 42);
  assert.throws(() => num('nope'), /expected a number/);
});
