'use client';

import { useEffect, useRef } from 'react';
import { useDrizzle } from '../lib/useDrizzle';
import { cityName } from '../lib/cities';
import { fmt, fmtAmount, fmtHourWeek, fmtMm } from '../lib/types';
import { CloseIcon, CloudIcon, CloudMark, CoinIcon, CrowdIcon, SeedIcon, SparkIcon, SunIcon } from './Sprites';
import { WorldMap } from './WorldMap';

export type DrawerTab = 'rounds' | 'how';

function shortHour(unixSecs: number): string {
  return new Date(unixSecs * 1000)
    .toLocaleString('en-US', {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
      hour12: false,
      timeZone: 'UTC',
    })
    .replace(',', '');
}

export function Drawer({
  tab,
  onTab,
  onClose,
}: {
  tab: DrawerTab | null;
  onTab: (tab: DrawerTab) => void;
  onClose: () => void;
}) {
  const { logs, config, cities, stats, keys, openVerify, contractId, mock } = useDrizzle();
  const panel = useRef<HTMLDivElement>(null);
  const open = tab !== null;

  useEffect(() => {
    if (!open) return;
    panel.current?.focus();
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open, onClose]);

  if (!open) return null;

  const threshold = config?.rain_threshold_tenths ?? 2;
  const evapPct = ((config?.evap_bps ?? 10) / 100).toFixed(2);
  const symbol = config?.symbol ?? 'DRIZZLE';

  const params: [string, string][] = config
    ? [
        ['Emission', `${fmtAmount(config.emission_per_round)} ${symbol} per round`],
        ['Rain threshold', `${fmtMm(threshold)} mm`],
        ['Rain weight cap', `${fmtMm(config.cap_tenths)} mm`],
        ['Evaporation', `${evapPct}% per dry city-round`],
        ['Seed', `${fmtAmount(config.seed_amount)} ${symbol}, once per account`],
        ['Keeper tip', `${fmtAmount(config.keeper_tip)} ${symbol} per tick`],
        ['Round', `${config.round_seconds}s${config.replay ? ` · one real hour replayed` : ' · one real hour'}`],
        ...(config.replay_hours > 0
          ? ([['Replay week', `${fmtHourWeek(config.first_hour, config.replay_hours)} (loops)`]] as [
              string,
              string,
            ][])
          : []),
        ['Contract', `${contractId}${mock ? ' (mock)' : ''}`],
      ]
    : [];

  return (
    <div className="sheet-backdrop" onMouseDown={onClose} role="presentation">
      <aside
        ref={panel}
        className="sheet drawer-sheet"
        role="dialog"
        aria-modal="true"
        aria-label="Rounds and rules"
        tabIndex={-1}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="sheet-head">
          <div className="tabs" role="tablist" aria-label="Rounds and rules">
            <button
              type="button"
              role="tab"
              aria-selected={tab === 'rounds'}
              className={`tab${tab === 'rounds' ? ' is-on' : ''}`}
              onClick={() => onTab('rounds')}
            >
              Rounds
            </button>
            <button
              type="button"
              role="tab"
              aria-selected={tab === 'how'}
              className={`tab${tab === 'how' ? ' is-on' : ''}`}
              onClick={() => onTab('how')}
            >
              How it works
            </button>
          </div>
          <button type="button" className="icon-btn" onClick={onClose} aria-label="Close panel">
            <CloseIcon />
          </button>
        </header>

        <div className="sheet-body">
          {tab === 'rounds' && (
            <section aria-label="Settled rounds">
              <p className="sheet-note">
                Every settled round, straight from the contract. Verify re-reads the same hour from Open-Meteo.
              </p>
              {logs.length === 0 ? (
                <p className="empty-line">No rounds settled yet.</p>
              ) : (
                <ol className="round-list">
                  {[...logs].reverse().map((log) => {
                    const wet = log.mm_tenths
                      .map((mm, index) => ({ mm, index }))
                      .filter((entry) => entry.mm >= threshold && entry.mm > 0);
                    const dry = log.mm_tenths.length - wet.length;
                    return (
                      <li key={log.round} className="round-row">
                        <div className="round-row-head">
                          <span className="round-id">Round {log.round}</span>
                          <span className="round-when">{shortHour(log.hour * 3600)}</span>
                          <button type="button" className="link-btn tiny" onClick={() => openVerify(log)}>
                            Verify
                          </button>
                        </div>
                        <div className="round-row-weather">
                          {wet.map((entry) => (
                            <span className="wet-chip" key={entry.index}>
                              <CloudIcon className="wet-chip-icon" />
                              {cityName(entry.index, cities[entry.index]?.name ?? '')} {fmtMm(entry.mm)} mm
                              <em>
                                +{fmt(log.minted[entry.index] ?? '0', 18, 1)}
                              </em>
                            </span>
                          ))}
                          <span className="dry-chip">
                            {dry} dry · −{evapPct}%
                          </span>
                        </div>
                      </li>
                    );
                  })}
                </ol>
              )}
            </section>
          )}

          {tab === 'how' && (
            <div className="how">
              <section className="how-block">
                <h2 className="sheet-title">
                  <CloudMark className="sheet-title-mark" />
                  Rain mints coins
                </h2>
                <ul className="mechanic">
                  <li>
                    <SeedIcon className="mech-icon" />
                    <span>
                      Plant {symbol} in a city. Coins sprout after the next settlement, so you never earn the
                      round you planted in.
                    </span>
                  </li>
                  <li>
                    <CloudIcon className="mech-icon is-rain" />
                    <span>
                      Wet cities split the round&apos;s emission by rain weight — min(mm,{' '}
                      {fmtMm(config?.cap_tenths ?? 50)}) — then pro-rata between their planters.
                    </span>
                  </li>
                  <li>
                    <CrowdIcon className="mech-icon is-crowd" />
                    <span>A wet city with no planters mints nothing and keeps its jackpot.</span>
                  </li>
                  <li>
                    <SunIcon className="mech-icon is-dry" />
                    <span>Every dry city-round evaporates {evapPct}% of its staked value, burned for good.</span>
                  </li>
                  <li>
                    <SparkIcon className="mech-icon is-act" />
                    <span>
                      Rain dance burns coins into a city&apos;s jackpot, paid out next time it rains there.
                      Anyone can tick a due round for {fmtAmount(config?.keeper_tip ?? '0')} {symbol}.
                    </span>
                  </li>
                </ul>
              </section>

              <section className="how-block">
                <h2 className="sheet-title">Parameters</h2>
                <dl className="param-list">
                  {params.map(([key, value]) => (
                    <div key={key}>
                      <dt>{key}</dt>
                      <dd>{value}</dd>
                    </div>
                  ))}
                </dl>
              </section>

              <section className="how-block">
                <h2 className="sheet-title">No admin keys</h2>
                {keys === null ? (
                  <p className="empty-line">Checking access keys over RPC…</p>
                ) : keys.length === 0 ? (
                  <p className="sheet-note">
                    <b>0 access keys.</b> Nobody can change this contract, not even the deployer — parameters,
                    cities and the pinned oracle hash are frozen at deploy.
                  </p>
                ) : (
                  <div>
                    <p className="sheet-note">
                      <b>{keys.length} access key{keys.length === 1 ? '' : 's'}</b> on {contractId}:
                    </p>
                    <ul className="key-list">
                      {keys.map((key) => (
                        <li key={key.public_key}>
                          <code>{key.public_key.length > 34 ? `${key.public_key.slice(0, 18)}…${key.public_key.slice(-10)}` : key.public_key}</code>
                        </li>
                      ))}
                    </ul>
                  </div>
                )}
              </section>

              {stats && (
                <section className="how-block">
                  <h2 className="sheet-title">Supply</h2>
                  <dl className="supply-stats">
                    <div>
                      <dt>Total supply</dt>
                      <dd>{fmtAmount(stats.total_supply)}</dd>
                    </div>
                    <div>
                      <dt>Minted</dt>
                      <dd>{fmtAmount(stats.minted)}</dd>
                    </div>
                    <div>
                      <dt>Burned</dt>
                      <dd>{fmtAmount(stats.burned)}</dd>
                    </div>
                    <div>
                      <dt>Danced</dt>
                      <dd>{fmtAmount(stats.danced)}</dd>
                    </div>
                    <div>
                      <dt>Seeded accounts</dt>
                      <dd>{stats.seeded_count}</dd>
                    </div>
                    <div>
                      <dt>Rounds settled</dt>
                      <dd>{stats.rounds_settled}</dd>
                    </div>
                  </dl>
                  <img
                    className="supply-chart"
                    src="/supply.svg"
                    alt="60-day supply simulation on real weather: supply stays flat, no runaway inflation and no death spiral"
                  />
                  <p className="sheet-note small">
                    Sixty simulated days on real weather, emission {fmtAmount(config?.emission_per_round ?? '0')}
                    {' '}
                    {symbol} per hour and {evapPct}% evaporation: supply holds flat with no runaway inflation.
                  </p>
                </section>
              )}

              <section className="how-block">
                <h2 className="sheet-title">Where the plots are</h2>
                <WorldMap />
                <p className="sheet-note small">
                  Twelve fixed cities, one real weather reading each. Pin size follows the stake; a pulse marks
                  last round&apos;s rain.
                </p>
              </section>

              <p className="how-foot">
                <CoinIcon /> Every reading is fetched by a pinned WASM inside an Intel TDX enclave and delivered
                to a keyless contract. Open any round and press Verify.
              </p>
            </div>
          )}
        </div>
      </aside>
    </div>
  );
}
