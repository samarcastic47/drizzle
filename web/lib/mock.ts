/**
 * In-memory mock of the Drizzle contract.
 *
 * It follows docs/interface.md and the settlement rules from HANDOFF.md §2.2 closely
 * enough to exercise the whole UI: real 60s rounds on the wall clock, deterministic
 * weather, pool accounting, per-user shares/pending, round logs and stats.
 *
 * Everything is bigint yocto arithmetic — no floats touch token amounts.
 */

import type {
  AccessKeyInfo,
  CityInfo,
  CityConfig,
  Config,
  RoundLog,
  RoundState,
  Stats,
  U128,
  UserView,
} from './types';
import { parseAmount } from './types';
import { ONE_DRIZZLE, minPlantAmount } from './round';
import type { DrizzleApi, StorageBounds } from './contract';
import replayWeek from '../data/replay-week.json';

const ROUND_SECONDS = 60;
const HISTORY_ROUNDS = 45;
/** The contract keeps only the newest 240 round logs. */
const LOG_CAPACITY = 240;
const MOCK_KEEPER = 'drizzle-dev.testnet';
const AUTO_KEEPER_GRACE_MS = 6_000;
const IN_FLIGHT_MS = 2_600;
const EVAP_BPS = 10;
const THRESHOLD_TENTHS = 2;
const CAP_TENTHS = 50;
const EMISSION = 100;
const SEED = 100;
const KEEPER_TIP = 1;

const CITIES: CityConfig[] = [
  { name: 'Bergen', lat_e4: 603900, lon_e4: 53200 },
  { name: 'Sao Paulo', lat_e4: -235500, lon_e4: -466300 },
  { name: 'Lagos', lat_e4: 65200, lon_e4: 33800 },
  { name: 'Singapore', lat_e4: 13500, lon_e4: 1038200 },
  { name: 'Tokyo', lat_e4: 356800, lon_e4: 1396900 },
  { name: 'Jakarta', lat_e4: -62000, lon_e4: 1068500 },
  { name: 'Mumbai', lat_e4: 190800, lon_e4: 728800 },
  { name: 'London', lat_e4: 515100, lon_e4: -1300 },
  { name: 'Lisbon', lat_e4: 387200, lon_e4: -91400 },
  { name: 'Seattle', lat_e4: 476100, lon_e4: -1223300 },
  { name: 'Cairo', lat_e4: 300400, lon_e4: 312400 },
  { name: 'Dubai', lat_e4: 252000, lon_e4: 552700 },
];

/** Plausible climate mix: Bergen/Singapore wet, Cairo/Dubai almost never. */
const RAIN_PROBABILITY = [0.33, 0.3, 0.24, 0.34, 0.26, 0.3, 0.2, 0.28, 0.18, 0.3, 0.03, 0.02];

/** Baseline crowd so the map looks alive before anyone connects. */
const BASELINE: { value: number; planters: number; jackpot?: number }[] = [
  { value: 3200, planters: 28 },
  { value: 5400, planters: 41 },
  { value: 1800, planters: 17 },
  { value: 7100, planters: 63, jackpot: 120 },
  { value: 6100, planters: 55 },
  { value: 2400, planters: 22 },
  { value: 2900, planters: 26 },
  { value: 4800, planters: 38 },
  { value: 1500, planters: 12 },
  { value: 2600, planters: 19 },
  { value: 900, planters: 7, jackpot: 800 },
  { value: 4200, planters: 9, jackpot: 4200 },
];

/**
 * Real Open-Meteo hourly precipitation (tenths of mm) for the replay week
 * 2026-09-24T00 → 2026-09-30T23 (168 hours), same city order. The contract loops these
 * hours every 168 rounds; the mock does the same so Verify re-fetches the same hour.
 */
const REPLAY_WEATHER = replayWeek as { first_hour: number; hours: number; tenths: number[][] };
const REPLAY_HOURS = REPLAY_WEATHER.hours;

const YOCTO = 10n ** 18n;
const drizzle = (n: number | string): bigint => BigInt(parseAmount(String(n)));

interface Pool {
  value: bigint;
  shares: bigint;
  jackpot: bigint;
  lastRainRound: number | null;
  lastMmTenths: number;
  botPlanters: number;
}

interface MockUser {
  balance: bigint;
  seeded: boolean;
  shares: bigint[];
  pending: bigint[];
  pendingRound: (number | null)[];
}

interface InternalLog {
  round: number;
  hour: number;
  mmTenths: number[];
  minted: bigint[];
  burned: bigint[];
  jackpotPaid: bigint[];
  sprouted: bigint[];
  keeper: string;
  keeperTip: bigint;
  outlayerRequestId: string;
  tickBlockHeight: number;
  settledTs: number;
}

const zeros = (n: number) => Array.from({ length: n }, () => 0n);

/** Deterministic 32-bit hash -> [0,1). Weather must be stable across reloads. */
function noise(round: number, salt: number): number {
  let h = (Math.imul(round, 0x9e3779b1) + Math.imul(salt, 0x85ebca6b)) >>> 0;
  h ^= h >>> 16;
  h = Math.imul(h, 0x7feb352d);
  h ^= h >>> 15;
  h = Math.imul(h, 0x846ca68b);
  h ^= h >>> 16;
  return (h >>> 0) / 4294967296;
}

/**
 * OutLayer request ids are plain integers. The mock mirrors the real replay keeper
 * (drizzle-dev.testnet), whose ids around 22023 map to real jobs.
 */
function mockRequestId(round: number): string {
  return String(22000 + round);
}

/** Blocks are ~1s; anchor the mock ledger near the current testnet height. */
const MOCK_BLOCK_BASE = 277_000_000;

/** One mock instance per contract id, so the Replay/Live toggle keeps separate state. */
const instances = new Map<string, MockApi>();

export function getMock(contractId: string, replay: boolean): MockApi {
  let api = instances.get(contractId);
  if (!api) {
    api = new MockApi(contractId, replay);
    instances.set(contractId, api);
  }
  return api;
}

export class MockApi implements DrizzleApi {
  readonly mode = 'mock' as const;
  private readonly genesis: number;
  private readonly firstHour: number;
  private readonly pools: Pool[];
  private readonly users = new Map<string, MockUser>();
  private readonly logs = new Map<number, InternalLog>();
  private readonly sproutRatios = new Map<string, { pendingIn: bigint; sharesOut: bigint }>();
  private nextRound = 0;
  private inFlight: { round: number; started_ts: number } | null = null;
  private minted = 0n;
  private burned = 0n;
  private danced = 0n;
  private seededCount = 0;
  private account: string | null = null;
  private timer: ReturnType<typeof setTimeout> | undefined;

  constructor(
    readonly contractId: string,
    private readonly replay: boolean,
  ) {
    const now = Math.floor(Date.now() / 1000);
    this.genesis = Math.floor(now / ROUND_SECONDS) * ROUND_SECONDS - HISTORY_ROUNDS * ROUND_SECONDS;
    this.firstHour = replay ? REPLAY_WEATHER.first_hour : Math.floor(this.genesis / 3600);
    this.pools = BASELINE.map((b) => {
      const value = drizzle(b.value);
      const jackpot = drizzle(b.jackpot ?? 0);
      this.minted += value + jackpot;
      this.burned += jackpot;
      this.danced += jackpot;
      return {
        value,
        shares: value,
        jackpot,
        lastRainRound: null,
        lastMmTenths: 0,
        botPlanters: b.planters,
      };
    });

    // Backfill history so the feed and the map have context on first paint.
    const target = Math.floor((now - this.genesis) / ROUND_SECONDS);
    while (this.nextRound < target) this.settleRound(this.nextRound);
  }

  /* ---------------- views ---------------- */

  setAccount(accountId: string | null) {
    this.account = accountId;
    if (accountId) this.user(accountId);
  }

  async getConfig(): Promise<Config> {
    return {
      name: 'Drizzle',
      symbol: 'DRIZZLE',
      decimals: 18,
      icon: null,
      cities: CITIES,
      emission_per_round: drizzle(EMISSION).toString(),
      evap_bps: EVAP_BPS,
      rain_threshold_tenths: THRESHOLD_TENTHS,
      cap_tenths: CAP_TENTHS,
      seed_amount: drizzle(SEED).toString(),
      keeper_tip: drizzle(KEEPER_TIP).toString(),
      round_seconds: ROUND_SECONDS,
      genesis_ts: this.genesis,
      first_hour: this.firstHour,
      replay: this.replay,
      replay_hours: this.replay ? REPLAY_HOURS : 0,
      oracle: {
        outlayer: 'outlayer.testnet',
        source: {
          type: 'github',
          repo: 'out-layer/rain-oracle',
          rev: 'a41f7c9',
          wasm_hash: 'sha256:9c1e5b7d2f4a8c06b3d91e77a0f5c2d8e6b4a1f39c7d05e8b2a64f1c9d3e7b05',
          build_target: 'wasm32-wasip2',
        },
        resource_limits: { instructions: 100_000_000, memory_mb: 64, timeout_s: 30 },
        deposit: '100000000000000000000000',
      },
    };
  }

  async getRoundState(): Promise<RoundState> {
    this.advance();
    return {
      next_round: this.nextRound,
      next_hour: this.roundHour(this.nextRound),
      oldest_round_log: Math.max(0, this.nextRound - LOG_CAPACITY),
      due_at: this.dueAt(this.nextRound),
      in_flight: this.inFlight ? { ...this.inFlight } : null,
      now: Math.floor(Date.now() / 1000),
    };
  }

  async getCities(): Promise<CityInfo[]> {
    this.advance();
    return this.pools.map((pool, index) => ({
      index,
      name: CITIES[index].name,
      lat_e4: CITIES[index].lat_e4,
      lon_e4: CITIES[index].lon_e4,
      value: pool.value.toString(),
      shares: pool.shares.toString(),
      pending_total: this.pendingTotal(index).toString(),
      jackpot: pool.jackpot.toString(),
      planters: pool.botPlanters + this.userPlanters(index),
      last_rain_round: pool.lastRainRound,
      last_mm_tenths: pool.lastMmTenths,
    }));
  }

  async getUser(accountId: string | null): Promise<UserView | null> {
    if (!accountId) return null;
    this.advance();
    const user = this.user(accountId);
    return {
      balance: user.balance.toString(),
      seeded: user.seeded,
      cities: this.pools.map((pool, index) => {
        const shares = user.shares[index];
        return {
          index,
          shares: shares.toString(),
          value: (pool.shares > 0n ? (shares * pool.value) / pool.shares : 0n).toString(),
          pending: user.pending[index].toString(),
          pending_round: user.pendingRound[index],
        };
      }),
    };
  }

  async getRoundLogs(from: number, limit: number): Promise<RoundLog[]> {
    this.advance();
    const out: RoundLog[] = [];
    const fromRound = Math.max(0, Math.floor(from));
    for (let r = fromRound; r < fromRound + Math.min(limit, 100); r += 1) {
      const log = this.logs.get(r);
      if (log) out.push(toRoundLog(log));
    }
    return out;
  }

  async getRoundLog(round: number): Promise<RoundLog | null> {
    this.advance();
    const log = this.logs.get(round);
    return log ? toRoundLog(log) : null;
  }

  /** The deployed contract's NEP-145 floor: 0.03199 NEAR. */
  async getStorageBounds(): Promise<StorageBounds> {
    return { min: '31990000000000000000000', max: null };
  }

  async getStats(): Promise<Stats> {
    this.advance();
    return {
      total_supply: this.totalSupply().toString(),
      minted: this.minted.toString(),
      burned: this.burned.toString(),
      danced: this.danced.toString(),
      seeded_count: this.seededCount,
      rounds_settled: this.nextRound,
    };
  }

  /** The whole point: the deployed account has no keys. */
  async viewAccessKeyList(): Promise<AccessKeyInfo[]> {
    return [];
  }

  async ftBalanceOf(accountId: string): Promise<U128> {
    this.advance();
    return this.user(accountId).balance.toString();
  }

  /* ---------------- actions ---------------- */

  async claimSeed(): Promise<void> {
    const user = this.requireUser();
    if (user.seeded) throw new Error('already seeded');
    user.seeded = true;
    user.balance += drizzle(SEED);
    this.minted += drizzle(SEED);
    this.seededCount += 1;
  }

  async plant(city: number, amount: U128): Promise<void> {
    this.assertCity(city);
    this.advance();
    const user = this.requireUser();
    const value = BigInt(amount);
    if (!this.replay && Date.now() >= this.dueAt(this.nextRound) * 1000) {
      throw new Error('round closing: wait for settlement');
    }
    if (value < ONE_DRIZZLE) throw new Error('plant amount below the 1 DRIZZLE minimum');
    const pool = this.pools[city];
    const min = minPlantAmount(
      { shares: pool.shares.toString(), value: pool.value.toString(), jackpot: pool.jackpot.toString() },
      drizzle(EMISSION).toString(),
    );
    if (value < min) throw new Error('city share price too high — plant a bigger amount');
    if (value <= 0n) throw new Error('amount must be positive');
    if (value > user.balance) throw new Error('insufficient balance');
    user.balance -= value;
    user.pending[city] += value;
    user.pendingRound[city] = this.nextRound;
  }

  async uproot(city: number, shares: U128 | null): Promise<void> {
    this.assertCity(city);
    this.advance();
    const user = this.requireUser();
    this.resolvePending(user, city);

    const pool = this.pools[city];
    const held = user.shares[city];
    const everything = shares === null;
    const wanted = everything ? held : BigInt(shares);
    if (wanted < 0n) throw new Error('invalid shares');
    if (wanted > held) throw new Error('insufficient shares');

    const payout = pool.shares > 0n ? (wanted * pool.value) / pool.shares : 0n;
    pool.value -= payout;
    pool.shares -= wanted;
    user.shares[city] = held - wanted;
    user.balance += payout;
    if (everything) {
      // "null = everything (all shares + unsprouted pending)", pending comes back 1:1.
      user.balance += user.pending[city];
      user.pending[city] = 0n;
      user.pendingRound[city] = null;
    }
  }

  async rainDance(city: number, amount: U128): Promise<void> {
    this.assertCity(city);
    const user = this.requireUser();
    const value = BigInt(amount);
    if (value <= 0n) throw new Error('amount must be positive');
    if (value > user.balance) throw new Error('insufficient balance');
    user.balance -= value;
    this.pools[city].jackpot += value;
    this.burned += value;
    this.danced += value;
  }

  async tick(): Promise<void> {
    this.advance();
    if (this.inFlight) throw new Error('tick in flight');
    if (Date.now() < this.dueAt(this.nextRound) * 1000) throw new Error('round not due');
    const round = this.nextRound;
    this.inFlight = { round, started_ts: Math.floor(Date.now() / 1000) };
    clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      this.timer = undefined;
      try {
        if (this.nextRound === round) this.settleRound(round);
      } finally {
        this.inFlight = null;
      }
    }, IN_FLIGHT_MS);
  }

  /* ---------------- internals ---------------- */

  /** Settle every round that is due and past the mock keeper's grace period. */
  private advance() {
    if (this.inFlight) return;
    while (Date.now() >= this.dueAt(this.nextRound) * 1000 + AUTO_KEEPER_GRACE_MS) {
      this.settleRound(this.nextRound);
    }
  }

  private dueAt(round: number): number {
    return this.genesis + (round + 1) * ROUND_SECONDS;
  }

  /** Replay loops the recorded week; live maps round r to hour first_hour + r. */
  private roundHour(round: number): number {
    return this.firstHour + (this.replay ? round % REPLAY_HOURS : round);
  }

  /** Real recorded hours for the replay window, synthetic weather past its end. */
  private weatherFor(round: number): number[] {
    const recorded = this.replay ? REPLAY_WEATHER.tenths[round % REPLAY_HOURS] : undefined;
    if (recorded) return CITIES.map((_, c) => recorded[c] ?? 0);
    const synthetic: number[] = [];
    for (let c = 0; c < CITIES.length; c += 1) {
      const front = noise(round, 4242);
      const desert = c >= CITIES.length - 2;
      const p = desert && round < HISTORY_ROUNDS ? 0 : RAIN_PROBABILITY[c] * (0.55 + 1.3 * front);
      const roll = noise(round, c);
      synthetic.push(roll < p ? Math.max(1, Math.round((0.2 + 4.8 * noise(round, 100 + c) ** 1.6) * 10)) : 0);
    }
    return synthetic;
  }

  private settleRound(round: number): void {
    const hour = this.roundHour(round);
    const mmTenths = this.weatherFor(round);

    const weights = mmTenths.map((mm) => (mm >= THRESHOLD_TENTHS ? Math.min(mm, CAP_TENTHS) : 0));
    const totalWeight = weights.reduce((a, b) => a + b, 0);
    const emission = drizzle(EMISSION);

    const minted = zeros(CITIES.length);
    const burned = zeros(CITIES.length);
    const jackpotPaid = zeros(CITIES.length);

    this.pools.forEach((pool, c) => {
      const w = weights[c];
      if (w > 0) {
        pool.lastRainRound = round;
        pool.lastMmTenths = mmTenths[c];
        if (pool.shares > 0n && totalWeight > 0) {
          const share = (emission * BigInt(w)) / BigInt(totalWeight);
          const pot = share + pool.jackpot;
          pool.value += pot;
          minted[c] = pot;
          jackpotPaid[c] = pool.jackpot;
          this.minted += pot;
          pool.jackpot = 0n;
        }
      } else {
        const burn = (pool.value * BigInt(EVAP_BPS)) / 10_000n;
        pool.value -= burn;
        burned[c] = burn;
        this.burned += burn;
      }
    });

    const sprouted = this.sprout(round);

    this.logs.set(round, {
      round,
      hour,
      mmTenths,
      minted,
      burned,
      jackpotPaid,
      sprouted,
      keeper: MOCK_KEEPER,
      keeperTip: drizzle(KEEPER_TIP),
      outlayerRequestId: mockRequestId(round),
      tickBlockHeight: MOCK_BLOCK_BASE + (this.dueAt(round) - this.genesis),
      settledTs: this.dueAt(round) + 1 + Math.floor(noise(round, 55) * 4),
    });
    this.minted += drizzle(KEEPER_TIP);
    this.logs.delete(round - LOG_CAPACITY);
    this.nextRound = round + 1;
  }

  /** Convert pending -> shares at the post-settlement price, per city. */
  private sprout(round: number): bigint[] {
    const sprouted = zeros(CITIES.length);
    this.pools.forEach((pool, c) => {
      let pendingIn = 0n;
      for (const user of this.users.values()) {
        if (user.pendingRound[c] === round && user.pending[c] > 0n) pendingIn += user.pending[c];
      }
      if (pendingIn === 0n) return;
      const sharesOut = pool.shares > 0n && pool.value > 0n ? (pendingIn * pool.shares) / pool.value : pendingIn;
      pool.shares += sharesOut;
      sprouted[c] = pendingIn;
      this.sproutRatios.set(`${round}:${c}`, { pendingIn, sharesOut });
      for (const user of this.users.values()) {
        if (user.pendingRound[c] === round && user.pending[c] > 0n) {
          user.shares[c] += (user.pending[c] * sharesOut) / pendingIn;
          user.pending[c] = 0n;
          user.pendingRound[c] = null;
        }
      }
    });
    return sprouted;
  }

  /** Lazy fallback in case a user still holds pending from an already-settled round. */
  private resolvePending(user: MockUser, city: number) {
    const round = user.pendingRound[city];
    if (round === null || round >= this.nextRound || user.pending[city] === 0n) return;
    const ratio = this.sproutRatios.get(`${round}:${city}`);
    const pending = user.pending[city];
    user.shares[city] += ratio && ratio.pendingIn > 0n ? (pending * ratio.sharesOut) / ratio.pendingIn : pending;
    user.pending[city] = 0n;
    user.pendingRound[city] = null;
  }

  private pendingTotal(city: number): bigint {
    let total = 0n;
    for (const user of this.users.values()) total += user.pending[city];
    return total;
  }

  private userPlanters(city: number): number {
    let count = 0;
    for (const user of this.users.values()) {
      if (user.shares[city] > 0n || user.pending[city] > 0n) count += 1;
    }
    return count;
  }

  private totalSupply(): bigint {
    let total = 0n;
    for (const pool of this.pools) total += pool.value;
    for (const user of this.users.values()) {
      total += user.balance;
      for (let c = 0; c < CITIES.length; c += 1) total += user.pending[c];
    }
    return total;
  }

  private user(accountId: string): MockUser {
    let user = this.users.get(accountId);
    if (!user) {
      user = {
        balance: 0n,
        seeded: false,
        shares: zeros(CITIES.length),
        pending: zeros(CITIES.length),
        pendingRound: Array.from({ length: CITIES.length }, () => null),
      };
      this.users.set(accountId, user);
    }
    return user;
  }

  private requireUser(): MockUser {
    if (!this.account) throw new Error('connect a wallet first');
    return this.user(this.account);
  }

  private assertCity(city: number) {
    if (!Number.isInteger(city) || city < 0 || city >= CITIES.length) throw new Error('invalid city');
  }
}

function toRoundLog(log: InternalLog): RoundLog {
  return {
    round: log.round,
    hour: log.hour,
    mm_tenths: [...log.mmTenths],
    minted: log.minted.map((v) => v.toString()),
    burned: log.burned.map((v) => v.toString()),
    jackpot_paid: log.jackpotPaid.map((v) => v.toString()),
    sprouted: log.sprouted.map((v) => v.toString()),
    keeper: log.keeper,
    keeper_tip: log.keeperTip.toString(),
    outlayer_request_id: log.outlayerRequestId,
    tick_block_height: log.tickBlockHeight,
    settled_ts: log.settledTs,
  };
}
