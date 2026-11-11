/** Value formatting for the keeper's log lines. */

const YOCTO_PER_NEAR = 10n ** 24n;

/** yoctoNEAR → "12.345678" (6 decimals, truncated). */
export function formatNear(yocto: bigint): string {
  const negative = yocto < 0n;
  const abs = negative ? -yocto : yocto;
  const whole = abs / YOCTO_PER_NEAR;
  const frac = (abs % YOCTO_PER_NEAR) / 10n ** 18n;
  return `${negative ? '-' : ''}${whole}.${frac.toString().padStart(6, '0')}`;
}

/** Unix **seconds** → "2026-11-11T12:00:05Z". */
export function isoSecond(unixSec: number): string {
  return new Date(unixSec * 1000).toISOString().replace(/\.\d{3}Z$/, 'Z');
}

/**
 * Unix **hour** → "2026-09-24T03:00Z".
 *
 * The contract reports `next_hour` as the hour *number* (497283), not as seconds,
 * so it has to be multiplied by 3600 before it reaches `Date`.
 */
export function isoHour(unixHour: number): string {
  // Hours are exact, so trim the seconds: "2026-09-24T03:00Z".
  return `${new Date(unixHour * 3600 * 1000).toISOString().slice(0, 16)}Z`;
}
