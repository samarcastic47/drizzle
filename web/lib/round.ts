/** Contract-rule helpers mirrored in the UI (docs/interface.md). */

import type { Config, RoundState, U128 } from './types';
import { toBig } from './types';

export const ONE_DRIZZLE = 10n ** 18n;
/** The pool must mint at least this many shares, else plant is rejected. */
const MIN_MINTED_SHARES = 1_000_000n;

/**
 * Smallest accepted plant amount.
 *
 * `amount >= 1e18`, and when the city already has shares the worst-case
 * post-settlement price must still mint >= 1e6 shares:
 *   ceil(1e6 * (value + emission_per_round + jackpot) / shares)
 */
export function minPlantAmount(
  city: { shares: U128; value: U128; jackpot: U128 },
  emissionPerRound: U128,
): bigint {
  const shares = toBig(city.shares);
  if (shares === 0n) return ONE_DRIZZLE;
  const worstCasePool = toBig(city.value) + toBig(emissionPerRound) + toBig(city.jackpot);
  const dustFloor = (MIN_MINTED_SHARES * worstCasePool + shares - 1n) / shares;
  return dustFloor > ONE_DRIZZLE ? dustFloor : ONE_DRIZZLE;
}

/**
 * Live rounds freeze plant/uproot once the round's hour is over and the keeper
 * has not settled yet. Replay rounds are exempt.
 *
 * Contract rounds are hourly, so the boundary is `(next_hour + 1) * 3600`.
 * Accelerated demo rounds (mock live, round_seconds < 1h) use the same freeze at
 * their own due time instead.
 */
export function isRoundClosing(
  config: Config | null,
  round: RoundState | null,
  nowSecs: number,
): boolean {
  if (!config || !round || config.replay) return false;
  if (config.round_seconds >= 3600) return nowSecs >= (round.next_hour + 1) * 3600;
  return nowSecs >= round.due_at;
}

export const ROUND_CLOSING_MESSAGE = 'round closing — settles in a moment';
