/**
 * Parsing of the two things a `tick()` leaves behind: NEP-297 event logs and the
 * transaction's failure payload.
 */

import {
  eventEnvelopeSchema,
  requestDataFieldSchema,
  requestDataSchema,
  roundEventSchema,
  roundFailedEventSchema,
} from './schema.js';
import { oneLine, safeJson } from './text.js';

export interface ParsedEvents {
  rounds: Array<{ round: number; hour: number; mm_tenths?: number[] }>;
  failures: Array<{ round: number; reason: string }>;
  requestId: string | null;
}

function tryParseJson(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return undefined;
  }
}

// near-sdk 5 derives event names from the enum variant with `rename_all =
// "snake_case"`, so `DrizzleEvents::Round` emits `"event":"round"`. docs/interface.md
// writes them as `drizzle_round`. Accept both spellings.
const ROUND_EVENT: Record<string, true> = { round: true, drizzle_round: true };
const ROUND_FAILED_EVENT: Record<string, true> = { round_failed: true, drizzle_round_failed: true };

/**
 * Pull NEP-297 events out of receipt logs:
 *  - `drizzle` standard: `round`, `round_failed`
 *  - `near-outlayer` standard: `execution_requested`, whose `data[0].request_data`
 *    is a JSON *string* containing the `request_id` the contract itself can never
 *    read (docs/notes/outlayer.md §3).
 */
export function parseEvents(logs: readonly string[]): ParsedEvents {
  const parsed: ParsedEvents = { rounds: [], failures: [], requestId: null };

  for (const line of logs) {
    const at = line.indexOf('EVENT_JSON:');
    if (at < 0) continue;
    const envelope = eventEnvelopeSchema.safeParse(tryParseJson(line.slice(at + 'EVENT_JSON:'.length)));
    if (!envelope.success) continue;

    for (const entry of envelope.data.data ?? []) {
      if (ROUND_EVENT[envelope.data.event] === true) {
        const round = roundEventSchema.safeParse(entry);
        if (round.success) {
          parsed.rounds.push({ round: round.data.round, hour: round.data.hour, mm_tenths: round.data.mm_tenths });
        }
      } else if (ROUND_FAILED_EVENT[envelope.data.event] === true) {
        const failed = roundFailedEventSchema.safeParse(entry);
        if (failed.success) parsed.failures.push({ round: failed.data.round, reason: failed.data.reason });
      }

      if (parsed.requestId === null) {
        const wrapped = requestDataFieldSchema.safeParse(entry);
        const rawRequestData = wrapped.success ? wrapped.data.request_data : undefined;
        const requestData =
          typeof rawRequestData === 'string' ? tryParseJson(rawRequestData) : (rawRequestData ?? entry);
        const withId = requestDataSchema.safeParse(requestData);
        if (withId.success && withId.data.request_id !== null) parsed.requestId = withId.data.request_id;
      }
    }
  }

  if (parsed.requestId === null) {
    // Belt and braces: some renderings of the event flatten the payload.
    for (const line of logs) {
      const match = /["']?request_id["']?\s*[:=]\s*["']?([0-9]+)/.exec(line);
      if (match) {
        parsed.requestId = match[1];
        break;
      }
    }
  }

  return parsed;
}

function collectStrings(node: unknown, out: string[], key = ''): void {
  if (typeof node === 'string') {
    if (/(ExecutionError|error_message|message|msg|reason)$/i.test(key)) out.push(node);
    return;
  }
  if (Array.isArray(node)) {
    for (const item of node) collectStrings(item, out, key);
    return;
  }
  if (typeof node === 'object' && node !== null) {
    for (const [childKey, value] of Object.entries(node)) collectStrings(value, out, childKey);
  }
}

/** Turn a NEAR `Failure` payload into a short human message ("round not due"). */
export function describeFailure(failure: unknown): string {
  const messages: string[] = [];
  collectStrings(failure, messages);
  const first = messages.find((message) => message.trim().length > 0);
  if (!first) return oneLine(safeJson(failure));
  return oneLine(first.replace(/^Smart contract panicked:\s*/, '').replace(/^panicked at [^:]*:\s*/, ''));
}

/** `{Failure: ...}` from a final execution status, as a message, if the tx failed. */
export function txFailure(status: unknown): string | undefined {
  if (typeof status !== 'object' || status === null || !('Failure' in status)) return undefined;
  return describeFailure(status.Failure);
}
