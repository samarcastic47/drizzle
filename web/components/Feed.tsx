'use client';

import { useDrizzle } from '../lib/useDrizzle';
import { fmtAmount, fmtHour, fmtMm, shortAccount, toBig } from '../lib/types';
import type { RoundLog } from '../lib/types';

interface Line {
  key: string;
  kind: 'rain' | 'empty';
  text: string;
}

function describeRound(log: RoundLog, names: string[], threshold: number, symbol: string) {
  const lines: Line[] = [];
  let dryCities = 0;
  let dryBurn = 0n;

  log.mm_tenths.forEach((mm, index) => {
    const name = names[index] ?? `City ${index}`;
    const minted = toBig(log.minted[index] ?? '0');
    const burned = toBig(log.burned[index] ?? '0');
    const rained = mm >= threshold && mm > 0;

    if (rained && minted > 0n) {
      const jackpot = toBig(log.jackpot_paid[index] ?? '0');
      lines.push({
        key: `rain-${index}`,
        kind: 'rain',
        text: `${name} ${fmtMm(mm)} mm → +${fmtAmount(minted)} ${symbol} to planters${
          jackpot > 0n ? ` (incl. ${fmtAmount(jackpot)} jackpot)` : ''
        }`,
      });
    } else if (rained) {
      lines.push({
        key: `empty-${index}`,
        kind: 'empty',
        text: `${name} ${fmtMm(mm)} mm → rained on an empty field (jackpot kept)`,
      });
    } else {
      dryCities += 1;
      dryBurn += burned;
    }
  });

  return { lines, dryCities, dryBurn };
}

export function Feed() {
  const { logs, cities, openVerify, config } = useDrizzle();
  const names = cities.map((city) => city.name);
  const threshold = config?.rain_threshold_tenths ?? 2;
  const evapPct = ((config?.evap_bps ?? 10) / 100).toFixed(2);
  const symbol = config?.symbol ?? 'DRIZZLE';

  return (
    <section className="card feed">
      <header className="feed-head">
        <h2>Round feed</h2>
        <p className="muted tiny">newest first · every settled round, straight from the contract</p>
      </header>

      {logs.length === 0 ? (
        <p className="muted">No rounds settled yet.</p>
      ) : (
        <ol className="feed-list">
          {[...logs].reverse().map((log) => {
            const { lines, dryCities, dryBurn } = describeRound(log, names, threshold, symbol);
            return (
              <li key={log.round} className="feed-round">
                <div className="feed-round-head">
                  <span className="feed-round-num">Round {log.round}</span>
                  <span className="feed-hour">{fmtHour(log.hour * 3600)}</span>
                  <button className="btn btn-tiny" onClick={() => openVerify(log)}>
                    Verify
                  </button>
                </div>
                <ul className="feed-lines">
                  {lines.map((line) => (
                    <li key={line.key} className={`feed-line is-${line.kind}`}>
                      {line.text}
                    </li>
                  ))}
                  <li className="feed-line is-dry">
                    {dryCities} cities dry → {evapPct}% evaporated
                    {dryBurn > 0n ? ` (−${fmtAmount(dryBurn)} ${symbol})` : ''}
                  </li>
                </ul>
                <p className="feed-foot">
                  keeper {shortAccount(log.keeper)} · +{fmtAmount(log.keeper_tip)} {symbol}
                  {log.outlayer_request_id
                    ? ` · ${shortAccount(log.outlayer_request_id, 8)}`
                    : ' · no request id'}
                </p>
              </li>
            );
          })}
        </ol>
      )}
    </section>
  );
}
