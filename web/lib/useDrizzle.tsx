'use client';

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import {
  RpcApi,
  apiSelection,
  isMissingContractError,
  type DrizzleApi,
  type StorageBounds,
} from './contract';
import { getMock } from './mock';
import { friendlyError } from './errors';
import { fetchCurrentRain } from './openmeteo';
import {
  MOCK_ACCOUNT_ID,
  disconnectWallet,
  getSelector,
  setWalletAccount,
  showWalletModal,
  walletSigner,
} from './wallet';
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

export type Mode = 'replay' | 'live';

const MOCK_ACCOUNT_KEY = 'drizzle:mock-account';
const POLL_ROUND_MS = 5_000;
const POLL_LOGS_MS = 10_000;
const POLL_WEATHER_MS = 5 * 60_000;

export interface DrizzleContextValue {
  booted: boolean;
  mode: Mode;
  setMode: (mode: Mode) => void;
  mock: boolean;
  contractId: string;
  replayContract: string;
  liveContract: string;
  config: Config | null;
  round: RoundState | null;
  roundAt: number;
  cities: CityInfo[];
  user: UserView | null;
  stats: Stats | null;
  storageBounds: StorageBounds | null;
  logs: RoundLog[];
  keys: AccessKeyInfo[] | null;
  rainNow: (number | null)[] | null;
  rainNowAt: number | null;
  accountId: string | null;
  walletReady: boolean;
  connecting: boolean;
  connect: () => void;
  disconnect: () => void;
  selected: number | null;
  setSelected: (index: number | null) => void;
  busy: string | null;
  error: string | null;
  notice: string | null;
  clearMessages: () => void;
  loadError: string | null;
  verify: RoundLog | null;
  openVerify: (log: RoundLog) => void;
  closeVerify: () => void;
  refresh: () => Promise<void>;
  claimSeed: () => Promise<void>;
  plant: (city: number, amount: U128) => Promise<void>;
  uproot: (city: number, shares: U128 | null) => Promise<void>;
  rainDance: (city: number, amount: U128) => Promise<void>;
  tick: () => Promise<void>;
}

const DrizzleContext = createContext<DrizzleContextValue | null>(null);

/** Land on the city that just got the most rain, so the card tells a story. */
function pickInitialCity(cities: CityInfo[], round: RoundState): number | null {
  if (cities.length === 0) return null;
  const justRained = cities.filter(
    (city) => city.last_rain_round !== null && city.last_rain_round === round.next_round - 1,
  );
  if (justRained.length > 0) {
    return justRained.reduce((best, city) => (city.last_mm_tenths > best.last_mm_tenths ? city : best)).index;
  }
  return cities.reduce((best, city) => (city.planters > best.planters ? city : best)).index;
}

function errorMessage(err: unknown): string {
  if (err instanceof Error) return err.message;
  return String(err);
}

export function DrizzleProvider({ children }: { children: ReactNode }) {
  const selection = useMemo(apiSelection, []);
  const { mock } = selection;
  const [mode, setMode] = useState<Mode>('replay');
  const contractId = mode === 'replay' ? selection.replayContract : selection.liveContract;

  const api = useMemo<DrizzleApi>(
    () => (mock ? getMock(contractId, mode === 'replay') : new RpcApi(contractId, walletSigner)),
    [mock, contractId, mode],
  );

  const [config, setConfig] = useState<Config | null>(null);
  const [round, setRound] = useState<RoundState | null>(null);
  const [roundAt, setRoundAt] = useState(() => Date.now());
  const [cities, setCities] = useState<CityInfo[]>([]);
  const [user, setUser] = useState<UserView | null>(null);
  const [stats, setStats] = useState<Stats | null>(null);
  const [storageBounds, setStorageBounds] = useState<StorageBounds | null>(null);
  const [logs, setLogs] = useState<RoundLog[]>([]);
  const [keys, setKeys] = useState<AccessKeyInfo[] | null>(null);
  const [rainNow, setRainNow] = useState<(number | null)[] | null>(null);
  const [rainNowAt, setRainNowAt] = useState<number | null>(null);
  const [booted, setBooted] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);

  const [accountId, setAccountId] = useState<string | null>(null);
  const [walletReady, setWalletReady] = useState(false);
  const [connecting, setConnecting] = useState(false);

  const [selected, setSelected] = useState<number | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [verify, setVerify] = useState<RoundLog | null>(null);

  const nextRoundRef = useRef(0);
  const oldestLogRef = useRef(0);
  const refreshRef = useRef<() => Promise<void>>(async () => {});

  /* ---------- wallet ---------- */

  useEffect(() => {
    if (!mock) {
      let cancelled = false;
      let unsubscribe: (() => void) | null = null;
      getSelector()
        .then((selector) => {
          if (cancelled) return;
          const sync = () => {
            const state = selector.store.getState();
            const active = state.accounts.find((a) => a.active) ?? state.accounts[0];
            setAccountId(active?.accountId ?? null);
            setWalletReady(true);
          };
          sync();
          const sub = selector.store.observable.subscribe(sync);
          unsubscribe = () => sub.unsubscribe();
        })
        .catch((err: unknown) => {
          if (cancelled) return;
          setWalletReady(true);
          setError(`wallet selector failed to load: ${errorMessage(err)}`);
        });
      return () => {
        cancelled = true;
        unsubscribe?.();
      };
    }

    const stored = typeof window === 'undefined' ? null : window.localStorage.getItem(MOCK_ACCOUNT_KEY);
    if (stored) setAccountId(stored);
    setWalletReady(true);
    return undefined;
  }, [mock]);

  useEffect(() => {
    setWalletAccount(accountId);
  }, [accountId]);

  /* ---------- contract reads ---------- */

  useEffect(() => {
    let cancelled = false;
    setBooted(false);
    setConfig(null);
    setRound(null);
    setCities([]);
    setUser(null);
    setStats(null);
    setStorageBounds(null);
    setLogs([]);
    setKeys(null);
    setLoadError(null);
    setVerify(null);
    setSelected(null);
    nextRoundRef.current = 0;

    const pullRound = async () => {
      const [roundState, cityList] = await Promise.all([api.getRoundState(), api.getCities()]);
      if (cancelled) return;
      nextRoundRef.current = roundState.next_round;
      oldestLogRef.current = roundState.oldest_round_log ?? 0;
      setRound(roundState);
      setRoundAt(Date.now());
      setCities(cityList);
      setSelected((current) => current ?? pickInitialCity(cityList, roundState));
    };

    const pullHistory = async () => {
      // The contract keeps only the newest 240 logs (oldest_round_log).
      const from = Math.max(oldestLogRef.current, nextRoundRef.current - 30);
      const [roundLogs, contractStats] = await Promise.all([api.getRoundLogs(from, 30), api.getStats()]);
      if (cancelled) return;
      setLogs(roundLogs);
      setStats(contractStats);
    };

    const pullUser = async () => {
      if (!accountId) {
        setUser(null);
        return;
      }
      const view = await api.getUser(accountId);
      if (!cancelled) setUser(view);
    };

    const pullAll = async () => {
      await pullRound();
      await pullHistory();
      await pullUser();
    };
    refreshRef.current = pullAll;

    (async () => {
      try {
        const cfg = await api.getConfig();
        if (cancelled) return;
        setConfig(cfg);
        await pullAll();
        if (cancelled) return;
        setBooted(true);
        api
          .getStorageBounds()
          .then((bounds) => {
            if (!cancelled) setStorageBounds(bounds);
          })
          .catch(() => {
            if (!cancelled) setStorageBounds(null);
          });
        api
          .viewAccessKeyList(api.contractId)
          .then((list) => {
            if (!cancelled) setKeys(list);
          })
          .catch(() => {
            if (!cancelled) setKeys(null);
          });
      } catch (err) {
        if (cancelled) return;
        setLoadError(isMissingContractError(err) ? 'not-deployed' : errorMessage(err));
        setBooted(true);
      }
    })();

    const roundTimer = setInterval(() => {
      pullRound().catch(() => undefined);
      pullUser().catch(() => undefined);
    }, POLL_ROUND_MS);
    const logTimer = setInterval(() => {
      pullHistory().catch(() => undefined);
    }, POLL_LOGS_MS);

    return () => {
      cancelled = true;
      clearInterval(roundTimer);
      clearInterval(logTimer);
      refreshRef.current = async () => {};
    };
  }, [api, accountId]);

  /* ---------- mock account mirrored into the data layer ---------- */

  useEffect(() => {
    api.setAccount(accountId);
  }, [api, accountId]);

  /* ---------- browser weather ---------- */

  useEffect(() => {
    if (!config || typeof window === 'undefined') return;
    let cancelled = false;
    const load = async () => {
      try {
        const rain = await fetchCurrentRain(config.cities);
        if (cancelled) return;
        setRainNow(rain);
        setRainNowAt(Date.now());
      } catch {
        // Offline / rate-limited: the pins simply show no droplet.
      }
    };
    load();
    const timer = setInterval(load, POLL_WEATHER_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [config]);

  /* ---------- wallet controls ---------- */

  const connect = useCallback(() => {
    setError(null);
    if (mock) {
      setAccountId(MOCK_ACCOUNT_ID);
      window.localStorage.setItem(MOCK_ACCOUNT_KEY, MOCK_ACCOUNT_ID);
      return;
    }
    setConnecting(true);
    showWalletModal(selection.replayContract)
      .catch((err: unknown) => setError(errorMessage(err)))
      .finally(() => setConnecting(false));
  }, [mock, selection.replayContract]);

  const disconnect = useCallback(() => {
    if (mock) {
      setAccountId(null);
      window.localStorage.removeItem(MOCK_ACCOUNT_KEY);
      return;
    }
    disconnectWallet().catch((err: unknown) => setError(errorMessage(err)));
  }, [mock]);

  /* ---------- transactions ---------- */

  const refresh = useCallback(async () => {
    await refreshRef.current();
  }, []);

  const run = useCallback(
    async (label: string, action: () => Promise<void>) => {
      setBusy(label);
      setError(null);
      setNotice(null);
      try {
        await action();
        setNotice(`${label} sent ✓`);
        const settle = Promise.withResolvers<void>();
        setTimeout(settle.resolve, 400);
        await settle.promise;
        await refreshRef.current();
      } catch (err) {
        setError(friendlyError(errorMessage(err)));
      } finally {
        setBusy(null);
      }
    },
    [],
  );

  const claimSeed = useCallback(() => run('Claim seed', () => api.claimSeed()), [api, run]);
  const plant = useCallback(
    (city: number, amount: U128) => run('Plant', () => api.plant(city, amount)),
    [api, run],
  );
  const uproot = useCallback(
    (city: number, shares: U128 | null) => run('Uproot', () => api.uproot(city, shares)),
    [api, run],
  );
  const rainDance = useCallback(
    (city: number, amount: U128) => run('Rain dance', () => api.rainDance(city, amount)),
    [api, run],
  );
  /** A tick settles a round a moment later, so refresh again once the callback lands. */
  const tick = useCallback(async () => {
    await run('Tick', () => api.tick());
    const settled = Promise.withResolvers<void>();
    setTimeout(settled.resolve, 3200);
    await settled.promise;
    await refreshRef.current();
  }, [api, run]);

  const clearMessages = useCallback(() => {
    setError(null);
    setNotice(null);
  }, []);

  const openVerify = useCallback((log: RoundLog) => setVerify(log), []);
  const closeVerify = useCallback(() => setVerify(null), []);

  const value = useMemo<DrizzleContextValue>(
    () => ({
      booted,
      mode,
      setMode,
      mock,
      contractId,
      replayContract: selection.replayContract,
      liveContract: selection.liveContract,
      config,
      round,
      roundAt,
      cities,
      user,
      stats,
      storageBounds,
      logs,
      keys,
      rainNow,
      rainNowAt,
      accountId,
      walletReady,
      connecting,
      connect,
      disconnect,
      selected,
      setSelected,
      busy,
      error,
      notice,
      clearMessages,
      loadError,
      verify,
      openVerify,
      closeVerify,
      refresh,
      claimSeed,
      plant,
      uproot,
      rainDance,
      tick,
    }),
    [
      booted,
      mode,
      mock,
      contractId,
      selection.replayContract,
      selection.liveContract,
      config,
      round,
      roundAt,
      cities,
      user,
      stats,
      storageBounds,
      logs,
      keys,
      rainNow,
      rainNowAt,
      accountId,
      walletReady,
      connecting,
      connect,
      disconnect,
      selected,
      busy,
      error,
      notice,
      clearMessages,
      loadError,
      verify,
      openVerify,
      closeVerify,
      refresh,
      claimSeed,
      plant,
      uproot,
      rainDance,
      tick,
    ],
  );

  return <DrizzleContext.Provider value={value}>{children}</DrizzleContext.Provider>;
}

export function useDrizzle(): DrizzleContextValue {
  const ctx = useContext(DrizzleContext);
  if (!ctx) throw new Error('useDrizzle must be used inside <DrizzleProvider>');
  return ctx;
}
