'use client';

import { useState } from 'react';
import { useDrizzle } from '../lib/useDrizzle';
import { fmt, fmtAmount, fmtHour, fmtHourWeek } from '../lib/types';

function shortKey(key: string): string {
  return key.length > 34 ? `${key.slice(0, 18)}…${key.slice(-10)}` : key;
}

export function About() {
  const { config, stats, keys, contractId, mock } = useDrizzle();
  const [chartMissing, setChartMissing] = useState(false);

  if (!config) return null;

  const params: [string, string][] = [
    ['emission / round', `${fmtAmount(config.emission_per_round)} ${config.symbol}`],
    ['evaporation', `${(config.evap_bps / 100).toFixed(2)}% per dry city-round`],
    ['rain threshold', `${(config.rain_threshold_tenths / 10).toFixed(1)} mm`],
    ['rain weight cap', `${(config.cap_tenths / 10).toFixed(1)} mm`],
    ['seed', `${fmtAmount(config.seed_amount)} ${config.symbol} once per account`],
    ['keeper tip', `${fmtAmount(config.keeper_tip)} ${config.symbol}`],
    ['round', `${config.round_seconds}s${config.replay ? ' (replay: one real hour per round)' : ''}`],
    ['genesis', fmtHour(config.genesis_ts)],
    ['round 0 hour', fmtHour(config.first_hour * 3600)],
    ...(config.replay_hours > 0
      ? ([
          ['replay window', `${config.replay_hours} h · week of ${fmtHourWeek(config.first_hour, config.replay_hours)} (loops)`],
        ] as [string, string][])
      : []),
    ['oracle account', config.oracle.outlayer],
    ['oracle deposit', `${fmt(config.oracle.deposit, 24, 4)} NEAR`],
    ['contract', `${contractId}${mock ? ' (mock)' : ''}`],
  ];

  return (
    <section className="about">
      <div className="card">
        <h2>How Drizzle works</h2>
        <ol className="mechanic">
          <li>
            <b>Plant</b> $DRIZZLE in a city. It sprouts after the next settlement, so you can never earn the round
            you planted in.
          </li>
          <li>
            <b>Rain mints.</b> Each round emits {fmtAmount(config.emission_per_round)} {config.symbol}, split
            across the cities that wet past {(config.rain_threshold_tenths / 10).toFixed(1)} mm, weighted by
            min(mm, {(config.cap_tenths / 10).toFixed(1)}) — then pro-rata to each city's planters.
          </li>
          <li>
            <b>Crowds dilute.</b> A wet city shares its rain between everyone staked there; a wet empty city pays
            nothing and keeps its jackpot.
          </li>
          <li>
            <b>Drought evaporates.</b> A dry city loses {(config.evap_bps / 100).toFixed(2)}% of its staked value,
            burned.
          </li>
          <li>
            <b>Rain dance</b> burns coins into a city's cloud jackpot, paid out the next time it rains there.
            Anyone can <b>tick</b> a due round and earn {fmtAmount(config.keeper_tip)} {config.symbol}.
          </li>
        </ol>
      </div>

      <div className="card">
        <h2>Parameters — immutable, read from get_config()</h2>
        <table className="param-table">
          <tbody>
            {params.map(([key, value]) => (
              <tr key={key}>
                <th scope="row">{key}</th>
                <td>{value}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <div className="card">
        <h2>No admin keys</h2>
        {keys === null ? (
          <p className="muted">Checking the contract account's access keys over RPC…</p>
        ) : keys.length === 0 ? (
          <div className="keyless">
            <b>0 access keys</b> — nobody can change this contract, not even the deployer.
            <p className="muted tiny">
              Read live from <code>view_access_key_list</code> for <code>{contractId}</code>. Parameters, cities
              and the pinned oracle hash are frozen at deploy time.
            </p>
          </div>
        ) : (
          <div className="keyless is-warn">
            <b>
              {keys.length} access key{keys.length === 1 ? '' : 's'} on {contractId}
            </b>
            <ul className="key-list">
              {keys.map((key) => (
                <li key={key.public_key}>
                  <code>{shortKey(key.public_key)}</code> · {describePermission(key.access_key.permission)}
                </li>
              ))}
            </ul>
          </div>
        )}
      </div>

      <div className="card">
        <h2>Supply</h2>
        {stats && (
          <dl className="stat-grid supply-stats">
            <div>
              <dt>total supply</dt>
              <dd>{fmtAmount(stats.total_supply)}</dd>
            </div>
            <div>
              <dt>minted</dt>
              <dd>{fmtAmount(stats.minted)}</dd>
            </div>
            <div>
              <dt>burned</dt>
              <dd>{fmtAmount(stats.burned)}</dd>
            </div>
            <div>
              <dt>danced</dt>
              <dd>{fmtAmount(stats.danced)}</dd>
            </div>
            <div>
              <dt>seeded accounts</dt>
              <dd>{stats.seeded_count}</dd>
            </div>
            <div>
              <dt>rounds settled</dt>
              <dd>{stats.rounds_settled}</dd>
            </div>
          </dl>
        )}
        {chartMissing ? (
          <p className="muted small">
            60-day supply simulation on real weather: supply stays flat, no runaway inflation and no death
            spiral (weekly 106.7k → 112.2k → 111.9k → 111.0k → 111.4k → 109.4k → 112.0k → 108.8k) with E =
            100/h and 0.10% evaporation.
          </p>
        ) : (
          <img
            className="supply-chart"
            src="/supply.svg"
            alt="60-day supply simulation on real weather: supply stays flat, no runaway inflation and no death spiral"
            onError={() => setChartMissing(true)}
          />
        )}
      </div>
    </section>
  );
}

function describePermission(permission: unknown): string {
  if (!permission || typeof permission !== 'object') return 'unknown permission';
  const record = permission as Record<string, unknown>;
  if ('FullAccess' in record) return 'full access ⚠️';
  if ('FunctionCall' in record) {
    const fc = record.FunctionCall as Record<string, unknown> | undefined;
    const receiver = typeof fc?.receiver_id === 'string' ? fc.receiver_id : '?';
    const allowance = fc?.allowance;
    return `function call → ${receiver}${typeof allowance === 'string' ? ` (allowance ${allowance} yocto)` : ''}`;
  }
  return Object.keys(record)[0] ?? 'unknown permission';
}
