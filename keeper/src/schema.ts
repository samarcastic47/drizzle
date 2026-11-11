import { z } from 'zod';

/**
 * Schemas for everything the keeper reads off chain. RPC payloads are untrusted:
 * every shape is parsed once at the boundary and then consumed as a typed value.
 */

/** u64 as it may appear over JSON-RPC: a JSON number, a decimal string, or a bigint. */
export const u64 = z.union([
  z.number().int().nonnegative(),
  z.bigint().nonnegative().transform(Number),
  z
    .string()
    .regex(/^[0-9]+$/)
    .transform(Number),
]);

/** `get_round_state()` (docs/interface.md). */
export const roundStateSchema = z.object({
  next_round: u64,
  next_hour: u64,
  due_at: u64,
  now: u64.nullish().transform((value) => value ?? 0),
  in_flight: z
    .object({ round: u64, started_ts: u64 })
    .nullish()
    .transform((value) => value ?? null),
});

/** The subset of `get_config()` the keeper needs; other fields pass through. */
export const configSchema = z.looseObject({
  oracle: z.looseObject({
    outlayer: z.string(),
    deposit: z.union([z.string(), z.number(), z.bigint()]),
  }),
  round_seconds: u64,
  replay: z.boolean().optional(),
});

/** `get_round_log()` — only the field the keeper falls back to. */
export const roundLogSchema = z.looseObject({
  outlayer_request_id: z
    .union([z.string(), z.number(), z.bigint()])
    .nullish()
    .transform((value) => (value === null || value === undefined ? null : String(value))),
});

/** A NEP-297 log line: `EVENT_JSON:{...}`. */
export const eventEnvelopeSchema = z.looseObject({
  event: z.string(),
  data: z.array(z.unknown()).optional(),
});

export const roundEventSchema = z.looseObject({
  round: u64,
  hour: u64,
  mm_tenths: z.array(u64).optional(),
});

export const roundFailedEventSchema = z.looseObject({
  round: u64,
  reason: z.string(),
});

/** OutLayer's `execution_requested` carries `request_data` as a JSON *string*. */
export const requestDataFieldSchema = z.looseObject({ request_data: z.unknown() });

export const requestDataSchema = z.looseObject({
  request_id: z
    .union([z.string(), z.number(), z.bigint(), z.null()])
    .optional()
    .transform((value) => (value === null || value === undefined ? null : String(value))),
});
