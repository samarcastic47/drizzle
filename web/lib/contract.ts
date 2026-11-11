/** Typed data layer: one interface, two implementations (mock / NEAR RPC). */

import { RpcError, ftBalanceOf, view, viewAccessKeyList } from './near';
import type {
  AccessKeyInfo,
  CityInfo,
  Config,
  RoundLog,
  RoundState,
  Stats,
  U128,
  UserView,
} from './types';

export const GAS_100_TGAS = '100000000000000';
export const CLAIM_SEED_DEPOSIT: U128 = '12500000000000000000000'; // 0.0125 NEAR
export const ONE_YOCTO: U128 = '1';

export interface TxRequest {
  contractId: string;
  methodName: string;
  args: Record<string, unknown>;
  deposit: U128;
}

/** Injected by the wallet layer so the data layer stays React-free. */
export interface Signer {
  accountId(): string | null;
  signFunctionCall(req: TxRequest): Promise<void>;
}

export interface DrizzleActions {
  claimSeed(): Promise<void>;
  plant(city: number, amount: U128): Promise<void>;
  uproot(city: number, shares: U128 | null): Promise<void>;
  rainDance(city: number, amount: U128): Promise<void>;
  tick(): Promise<void>;
}

export interface DrizzleApi extends DrizzleActions {
  readonly contractId: string;
  readonly mode: 'mock' | 'rpc';
  setAccount(accountId: string | null): void;
  getConfig(): Promise<Config>;
  getRoundState(): Promise<RoundState>;
  getCities(): Promise<CityInfo[]>;
  getUser(accountId: string | null): Promise<UserView | null>;
  getRoundLogs(from: number, limit: number): Promise<RoundLog[]>;
  getRoundLog(round: number): Promise<RoundLog | null>;
  getStats(): Promise<Stats>;
  viewAccessKeyList(accountId: string): Promise<AccessKeyInfo[]>;
  ftBalanceOf(accountId: string): Promise<U128>;
}

export class RpcApi implements DrizzleApi {
  readonly mode = 'rpc' as const;
  private account: string | null = null;
  private config: Config | null = null;

  constructor(
    readonly contractId: string,
    private readonly signer: Signer,
  ) {}

  setAccount(accountId: string | null) {
    this.account = accountId;
  }

  async getConfig(): Promise<Config> {
    if (!this.config) this.config = await view<Config>(this.contractId, 'get_config');
    return this.config;
  }

  getRoundState() {
    return view<RoundState>(this.contractId, 'get_round_state');
  }

  getCities() {
    return view<CityInfo[]>(this.contractId, 'get_cities');
  }

  getUser(accountId: string | null): Promise<UserView | null> {
    if (!accountId) return Promise.resolve(null);
    return view<UserView>(this.contractId, 'get_user', { account_id: accountId });
  }

  getRoundLogs(from: number, limit: number) {
    return view<RoundLog[]>(this.contractId, 'get_round_logs', { from, limit: Math.min(limit, 100) });
  }

  async getRoundLog(round: number) {
    const log = await view<RoundLog | null>(this.contractId, 'get_round_log', { round });
    return log ?? null;
  }

  getStats() {
    return view<Stats>(this.contractId, 'get_stats');
  }

  viewAccessKeyList(accountId: string) {
    return viewAccessKeyList(accountId);
  }

  ftBalanceOf(accountId: string) {
    return ftBalanceOf(this.contractId, accountId);
  }

  claimSeed() {
    return this.sign({ methodName: 'claim_seed', args: {}, deposit: CLAIM_SEED_DEPOSIT });
  }

  plant(city: number, amount: U128) {
    return this.sign({ methodName: 'plant', args: { city, amount }, deposit: ONE_YOCTO });
  }

  uproot(city: number, shares: U128 | null) {
    return this.sign({ methodName: 'uproot', args: { city, shares }, deposit: ONE_YOCTO });
  }

  rainDance(city: number, amount: U128) {
    return this.sign({ methodName: 'rain_dance', args: { city, amount }, deposit: ONE_YOCTO });
  }

  async tick() {
    const config = await this.getConfig();
    return this.sign({ methodName: 'tick', args: {}, deposit: config.oracle.deposit });
  }

  private async sign({ methodName, args, deposit }: Omit<TxRequest, 'contractId'>) {
    if (!this.account && !this.signer.accountId()) throw new Error('connect a wallet first');
    await this.signer.signFunctionCall({ contractId: this.contractId, methodName, args, deposit });
  }
}

export interface ApiSelection {
  replayContract: string;
  liveContract: string;
  mock: boolean;
}

export function apiSelection(): ApiSelection {
  return {
    replayContract: process.env.NEXT_PUBLIC_REPLAY_CONTRACT || 'drizzle-replay.testnet',
    liveContract: process.env.NEXT_PUBLIC_LIVE_CONTRACT || 'drizzle.testnet',
    mock: process.env.NEXT_PUBLIC_MOCK === '1',
  };
}

/**
 * True when the account has no Drizzle contract behind it: unknown account, no code,
 * or a contract that does not answer `get_config` (METHOD_NOT_FOUND).
 */
export function isMissingContractError(err: unknown): boolean {
  if (!(err instanceof RpcError)) return false;
  return /does not exist|Unknown account|AccountDoesNotExist|CodeDoesNotExist|ContractNotDeployed|Method not found|METHOD_NOT_FOUND|Contract method is not found/i.test(
    `${err.message} ${err.code ?? ''}`,
  );
}
