'use client';

import { useDrizzle } from '../lib/useDrizzle';
import { useNow } from '../lib/useNow';
import { fmtCountdown, fmtHour, fmtHourWeek, fmt } from '../lib/types';

export function RoundBar() {
  const { round, roundAt, config, mode, setMode, tick, busy, loadError } = useDrizzle();
  const now = useNow(1000);

  const inFlight = round?.in_flight ?? null;
  const skew = round ? round.now + (now - roundAt) / 1000 : 0;
  const remaining = round ? round.due_at - skew : 0;
  const due = round !== null && remaining <= 0;
  const roundSeconds = config?.round_seconds ?? 60;
  const progress = round ? Math.min(1, Math.max(0, 1 - remaining / roundSeconds)) : 0;

  return (
    <section className="round-bar" aria-live="polite">
      <div className="round-toggle" role="group" aria-label="Contract deployment">
        <button
          className={`seg ${mode === 'replay' ? 'is-on' : ''}`}
          onClick={() => setMode('replay')}
          aria-pressed={mode === 'replay'}
        >
          Replay
        </button>
        <button
          className={`seg ${mode === 'live' ? 'is-on' : ''}`}
          onClick={() => setMode('live')}
          aria-pressed={mode === 'live'}
        >
          Live
        </button>
      </div>

      <div className="round-main">
        {round ? (
          <div className="round-line">
            <strong className="round-number">Round {round.next_round}</strong>
            {inFlight ? (
              <span className="round-status is-busy">
                <span className="spinner" aria-hidden /> oracle running in OutLayer TEE…
              </span>
            ) : due ? (
              <span className="round-status is-waiting">waiting for a keeper</span>
            ) : (
              <span className="round-status">
                settles in <b className="mono">{fmtCountdown(remaining)}</b>
              </span>
            )}
          </div>
        ) : (
          <div className="round-line">
            <strong className="round-number">
              {loadError ? 'No round — contract not reachable' : 'Loading round…'}
            </strong>
          </div>
        )}

        {round && config?.replay && (
          <p className="round-sub">
            simulating {fmtHour(round.next_hour * 3600)} · {config.round_seconds}s rounds
            {config.replay_hours > 0 && (
              <> · replaying the week of {fmtHourWeek(config.first_hour, config.replay_hours)} (loop)</>
            )}
          </p>
        )}
        {round && config && !config.replay && (
          <p className="round-sub">settles {fmtHour(round.due_at)} · hourly live data</p>
        )}

        <div className="round-progress" aria-hidden>
          <span style={{ width: `${progress * 100}%` }} className={due ? 'is-due' : ''} />
        </div>
      </div>

      <div className="round-actions">
        {round && due && !inFlight && (
          <button className="btn btn-rain" onClick={tick} disabled={busy !== null} title="Anyone can tick. The caller earns the keeper tip.">
            {busy === 'Tick' ? 'Ticking…' : `Tick (+${fmt(config?.keeper_tip ?? '0')} DRIZZLE)`}
          </button>
        )}
        {inFlight && <span className="chip chip-busy">in flight · round {inFlight.round}</span>}
        {!round && !loadError && <span className="chip">—</span>}
      </div>
    </section>
  );
}
