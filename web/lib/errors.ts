/** Contract panics -> short, human toasts. */

const MAPPINGS: [RegExp, string][] = [
  [/round closing|round_closing/i, 'round closing: wait for settlement'],
  [/share price too high|worst.?case|1e6 shares|dust/i, 'city share price too high — plant a bigger amount'],
  [/below the 1 drizzle minimum|amount below|1 drizzle minimum|min(imum)? plant/i, 'plant amount below the 1 DRIZZLE minimum'],
  [/already seeded/i, 'already seeded — this account already claimed its seed'],
  [/round not due/i, 'round not due yet'],
  [/tick in flight/i, 'a tick is already in flight'],
  [/remaining stake below minimum|uproot all/i, 'remaining stake below minimum — uproot all instead'],
];

/** Match a raw wallet/RPC/contract error against the known panics. */
export function friendlyError(message: string): string {
  for (const [pattern, text] of MAPPINGS) {
    if (pattern.test(message)) return text;
  }
  return message;
}
