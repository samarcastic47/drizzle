import test from 'node:test';
import assert from 'node:assert/strict';

import { parseEvents, txFailure } from './events.js';

const eventLine = (payload: unknown): string => `EVENT_JSON:${JSON.stringify(payload)}`;

test('extracts the OutLayer request id from execution_requested', () => {
  const logs = [
    'tick called',
    eventLine({
      standard: 'near-outlayer',
      version: '1.0.0',
      event: 'execution_requested',
      data: [{ request_data: JSON.stringify({ request_id: 22023, sender_id: 'keeper.testnet' }) }],
    }),
  ];
  assert.equal(parseEvents(logs).requestId, '22023');
});

test('accepts the near-sdk snake_case event names', () => {
  const logs = [
    eventLine({
      standard: 'drizzle',
      version: '1.0.0',
      event: 'round',
      data: [{ round: 12, hour: 497280 + 12 * 3600, mm_tenths: [0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0] }],
    }),
    eventLine({ standard: 'drizzle', version: '1.0.0', event: 'round_failed', data: [{ round: 13, reason: 'outlayer returned null' }] }),
  ];
  const parsed = parseEvents(logs);
  assert.equal(parsed.rounds.length, 1);
  assert.equal(parsed.rounds[0].round, 12);
  assert.equal(parsed.rounds[0].mm_tenths?.[2], 3);
  assert.deepEqual(parsed.failures, [{ round: 13, reason: 'outlayer returned null' }]);
});

test('accepts the documented drizzle_round / drizzle_round_failed names', () => {
  const logs = [
    eventLine({ standard: 'drizzle', version: '1.0.0', event: 'drizzle_round', data: [{ round: 4, hour: 497280 }] }),
    eventLine({ standard: 'drizzle', version: '1.0.0', event: 'drizzle_round_failed', data: [{ round: 5, reason: 'bad payload' }] }),
  ];
  const parsed = parseEvents(logs);
  assert.equal(parsed.rounds[0].round, 4);
  assert.equal(parsed.failures[0].reason, 'bad payload');
});

test('ignores unrelated and malformed logs', () => {
  const parsed = parseEvents(['plain log', 'EVENT_JSON:{not json', eventLine({ standard: 'nep141', event: 'ft_mint', data: [] })]);
  assert.deepEqual(parsed, { rounds: [], failures: [], requestId: null });
});

test('txFailure surfaces the contract message', () => {
  const failure = {
    ActionError: { index: 0, kind: { FunctionCallError: { ExecutionError: 'Smart contract panicked: round not due' } } },
  };
  assert.equal(txFailure({ Failure: failure }), 'round not due');
  assert.equal(txFailure({ SuccessValue: '' }), undefined);
  assert.equal(txFailure('NotStarted'), undefined);
});
