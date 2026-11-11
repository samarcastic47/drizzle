/**
 * Types mirroring docs/interface.md exactly.
 * All U128 amounts are JSON decimal strings; timestamps are unix seconds.
 */

export type U128 = string;

export interface CityConfig {
  name: string;
  lat_e4: number;
  lon_e4: number;
}

export interface OracleConfig {
  outlayer: string;
  /** JSON pinned by the oracle WASM hash. Shape owned by the contract, opaque here. */
  source: unknown;
  resource_limits: unknown;
  deposit: U128;
}

export interface Config {
  name: string;
  symbol: string;
  decimals: number;
  icon: string | null;
  cities: CityConfig[];
  emission_per_round: U128;
  evap_bps: number;
  rain_threshold_tenths: number;
  cap_tenths: number;
  seed_amount: U128;
  keeper_tip: U128;
  round_seconds: number;
  genesis_ts: number;
  first_hour: number;
  replay: boolean;
  oracle: OracleConfig;
}

export interface InFlight {
  round: number;
  started_ts: number;
}

export interface RoundState {
  next_round: number;
  next_hour: number;
  due_at: number;
  in_flight: InFlight | null;
  now: number;
}

export interface CityInfo {
  index: number;
  name: string;
  lat_e4: number;
  lon_e4: number;
  value: U128;
  shares: U128;
  pending_total: U128;
  jackpot: U128;
  planters: number;
  last_rain_round: number | null;
  last_mm_tenths: number;
}

export interface UserCity {
  index: number;
  shares: U128;
  value: U128;
  pending: U128;
  pending_round: number | null;
}

export interface UserView {
  balance: U128;
  seeded: boolean;
  cities: UserCity[];
}

export interface RoundLog {
  round: number;
  hour: number;
  mm_tenths: number[];
  minted: U128[];
  burned: U128[];
  jackpot_paid: U128[];
  sprouted: U128[];
  keeper: string;
  keeper_tip: U128;
  /** Echoed by the oracle from NEAR_REQUEST_ID (part of the attested output). */
  outlayer_request_id: string | null;
  /** Block of the tick() transaction that requested this reading. */
  tick_block_height: number;
  settled_ts: number;
}

export interface Stats {
  total_supply: U128;
  minted: U128;
  burned: U128;
  danced: U128;
  seeded_count: number;
  rounds_settled: number;
}

export interface AccessKeyInfo {
  public_key: string;
  access_key: {
    nonce: number;
    permission: unknown;
  };
}

/* ---------- amount helpers: all bigint, never float ---------- */

/** Number of decimals for $DRIZZLE (from get_config().decimals, but 18 is fixed). */
export const DECIMALS = 18;

export function toBig(v: U128 | bigint | number): bigint {
  if (typeof v === 'bigint') return v;
  if (typeof v === 'number') {
    if (!Number.isFinite(v)) return 0n;
    return BigInt(Math.trunc(v));
  }
  const s = String(v).trim();
  if (!s) return 0n;
  if (!/^-?\d+$/.test(s)) {
    // Tolerate stray decimal strings coming from a mock/JSON source.
    return BigInt(parseAmount(s));
  }
  return BigInt(s);
}

/**
 * Format a yocto amount with thousands separators and no trailing zeros.
 * fmt("1234500000000000000000") === "1,234.5"
 */
export function fmt(value: U128 | bigint | number, decimals = DECIMALS, maxFrac = 2): string {
  const raw = toBig(value);
  const negative = raw < 0n;
  const abs = negative ? -raw : raw;
  const divisor = 10n ** BigInt(decimals);
  const whole = abs / divisor;
  let frac = (abs % divisor).toString().padStart(decimals, '0');

  if (maxFrac <= 0) {
    // Round-half-up on the whole part, then re-check for carry.
    const roundDigit = frac[0] ? Number(frac[0]) : 0;
    const rounded = roundDigit >= 5 ? whole + 1n : whole;
    return `${negative ? '-' : ''}${group(rounded)}`;
  }

  // Round-half-up at maxFrac.
  let kept = frac.slice(0, maxFrac);
  const nextDigit = frac.length > maxFrac ? Number(frac[maxFrac]) : 0;
  let wholeOut = whole;
  if (nextDigit >= 5) {
    let carried = BigInt(kept || '0') + 1n;
    if (carried >= 10n ** BigInt(maxFrac)) {
      wholeOut += 1n;
      carried = 0n;
    }
    kept = carried.toString().padStart(maxFrac, '0');
  }
  kept = kept.replace(/0+$/, '');
  const sign = negative ? '-' : '';
  const body = kept ? `${group(wholeOut)}.${kept}` : group(wholeOut);
  return `${sign}${body}`;
}

/** Compact display: values under 0.005 render as "<0.01" instead of "0". */
export function fmtAmount(value: U128 | bigint | number): string {
  const out = fmt(value);
  if (out === '0') {
    const raw = toBig(value);
    if (raw > 0n) return '<0.01';
    if (raw < 0n) return '>-0.01';
  }
  return out;
}

function group(n: bigint): string {
  return n.toString().replace(/\B(?=(\d{3})+(?!\d))/g, ',');
}

/**
 * Parse a human decimal string ("1,234.5") into a yocto U128 string.
 * Throws on malformed input or more than 18 fractional digits.
 */
export function parseAmount(input: string, decimals = DECIMALS): U128 {
  const s = input.replace(/[,\s_]/g, '').replace(/^\$/, '');
  if (!/^\d*(\.\d*)?$/.test(s) || s === '' || s === '.') {
    throw new Error(`invalid amount: "${input}"`);
  }
  const [intPart, fracPart = ''] = s.split('.');
  if (fracPart.length > decimals) {
    throw new Error(`at most ${decimals} decimals`);
  }
  const padded = fracPart.padEnd(decimals, '0');
  const yocto = `${intPart || '0'}${padded}`.replace(/^0+(?=\d)/, '');
  return yocto === '' ? '0' : yocto;
}

/** True when the string parses to a positive amount. */
export function isPositive(input: string): boolean {
  try {
    return BigInt(parseAmount(input)) > 0n;
  } catch {
    return false;
  }
}

/** Visual-only scaling (map pin radii). Never use for balances. */
export function approx(value: U128 | bigint): number {
  return Number(toBig(value) / 10n ** 12n) / 1e6;
}

/* ---------- misc formatters ---------- */

export function shortAccount(id: string, keep = 5): string {
  if (id.length <= keep * 2 + 1) return id;
  return `${id.slice(0, keep + 1)}…${id.slice(-keep)}`;
}

/** tenths of a millimetre -> "2.1" */
export function fmtMm(tenths: number): string {
  return (Math.round(tenths) / 10).toFixed(1);
}

/** unix seconds -> "Nov 11, 14:00 UTC" */
export function fmtHour(unixSecs: number): string {
  return `${new Date(unixSecs * 1000).toLocaleString('en-US', {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    timeZone: 'UTC',
    hour12: false,
  })} UTC`;
}

/** unix seconds -> "2026-11-11T14:00" (Open-Meteo hour form) */
export function fmtHourIso(unixSecs: number): string {
  return `${new Date(unixSecs * 1000).toISOString().slice(0, 13)}:00`;
}

/** seconds -> "mm:ss" */
export function fmtCountdown(secs: number): string {
  const s = Math.max(0, Math.floor(secs));
  const m = Math.floor(s / 60);
  const rest = s % 60;
  return `${m}:${rest.toString().padStart(2, '0')}`;
}
