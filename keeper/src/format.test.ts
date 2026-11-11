import test from 'node:test';
import assert from 'node:assert/strict';

import { formatNear, isoHour, isoSecond } from './format.js';

test('formats a unix hour as a UTC timestamp', () => {
  // first_hour of the replay contract: 497280 == 2026-09-24T00:00Z
  assert.equal(isoHour(497280), '2026-09-24T00:00Z');
  assert.equal(isoHour(497283), '2026-09-24T03:00Z');
  assert.equal(isoHour(0), '1970-01-01T00:00Z');
});

test('formats unix seconds as a UTC timestamp', () => {
  assert.equal(isoSecond(497283 * 3600), '2026-09-24T03:00:00Z');
});

test('formats yoctoNEAR with six decimals', () => {
  assert.equal(formatNear(0n), '0.000000');
  assert.equal(formatNear(10n ** 24n), '1.000000');
  assert.equal(formatNear(2_396_000_000_000_000_000_000n), '0.002396');
  assert.equal(formatNear(1_234_567_890_000_000_000_000_001n), '1.234567');
  assert.equal(formatNear(-1_500_000_000_000_000_000_000n), '-0.001500');
});
