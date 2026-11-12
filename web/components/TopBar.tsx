'use client';

import { useEffect, useRef } from 'react';
import { useDrizzle } from '../lib/useDrizzle';
import { useNow } from '../lib/useNow';
import { fmtAmount, fmtCountdown, fmt, shortAccount } from '../lib/types';
import { prefersReducedMotion } from '../lib/fx';
import { CloudMark, CoinIcon } from './Sprites';

export function TopBar({ onOpenRules }: { onOpenRules: () => void }) {
  const {
    mode,
    setMode,
    round,
    roundAt,
    config,
    tick,
    busy,
    accountId,
    user,
    connect,
    disconnect,
    connecting,
    mock,
  } = useDrizzle();
  const now = useNow(1000);
  const pillRef = useRef<HTMLDivElement>(null);
  const seenRound = useRef<number | null>(null);

  const skew = round ? round.now + (now - roundAt) / 1000 : 0;
  const remaining = round ? round.due_at - skew : 0;
  const due = round !== null && remaining <= 0;
  const roundSeconds = config?.round_seconds ?? 60;
  const progress = round ? Math.min(1, Math.max(0, 1 - remaining / roundSeconds)) : 0;
  const inFlight = round?.in_flight ?? null;

  // A settled round bumps next_round; the pill tells you so once.
  useEffect(() => {
    if (!round) return;
    if (seenRound.current === null) {
      seenRound.current = round.next_round;
      return;
    }
    if (round.next_round <= seenRound.current) return;
    seenRound.current = round.next_round;
    const pill = pillRef.current;
    if (!pill || prefersReducedMotion()) return;
    pill.animate(
      [
        { transform: 'scale(1)', borderColor: 'var(--parchment)' },
        { transform: 'scale(1.045)', borderColor: 'var(--rain)' },
        { transform: 'scale(1)', borderColor: 'var(--parchment)' },
      ],
      { duration: 560, easing: 'cubic-bezier(.16,1,.3,1)' },
    );
  }, [round]);

  return (
    <header className="topbar">
      <a className="brand" href="#board" aria-label="Drizzle — back to the plots">
        <CloudMark className="brand-mark" />
        <span className="wordmark">Drizzle</span>
      </a>

      <div className="bar-center">
        <div className="segmented" role="group" aria-label="Which contract to watch">
          <button
            type="button"
            className={`seg${mode === 'replay' ? ' is-on' : ''}`}
            aria-pressed={mode === 'replay'}
            onClick={() => setMode('replay')}
          >
            Replay
          </button>
          <button
            type="button"
            className={`seg${mode === 'live' ? ' is-on' : ''}`}
            aria-pressed={mode === 'live'}
            onClick={() => setMode('live')}
          >
            Live
          </button>
        </div>

        <div className="round-pill" ref={pillRef} aria-live="polite">
          <div className="round-pill-line">
            {round ? (
              <>
                <span className="round-num">Round {round.next_round}</span>
                <span className="round-sep" aria-hidden>
                  ·
                </span>
                <span className={`round-state${inFlight ? ' is-reading' : due ? ' is-waiting' : ''}`}>
                  {inFlight ? (
                    <>
                      <span className="state-long">oracle reading the sky…</span>
                      <span className="state-short">oracle reading…</span>
                    </>
                  ) : due ? (
                    <>
                      <span className="state-long">waiting for a keeper</span>
                      <span className="state-short">keeper needed</span>
                    </>
                  ) : (
                    fmtCountdown(remaining)
                  )}
                </span>
              </>
            ) : (
              <span className="round-state">reading the round…</span>
            )}
          </div>
          <span className="round-fill" aria-hidden>
            <span style={{ width: `${progress * 100}%` }} />
          </span>
        </div>

        {round && due && !inFlight && (
          <button
            type="button"
            className="btn btn-act btn-tick"
            onClick={tick}
            disabled={busy !== null}
            title={`Anyone can tick. The caller earns ${fmt(config?.keeper_tip ?? '0')} DRIZZLE.`}
          >
            {busy === 'Tick' ? 'Ticking…' : 'Tick +1'}
          </button>
        )}
      </div>

      <div className="bar-right">
        <button type="button" className="link-btn rules-btn" onClick={onOpenRules}>
          Rounds &amp; rules
        </button>

        {accountId ? (
          <button
            type="button"
            className="wallet-chip"
            onClick={disconnect}
            aria-label={`Disconnect wallet ${accountId}`}
            title={mock ? 'Leave the mock account' : 'Disconnect wallet'}
          >
            <CoinIcon className="chip-coin" />
            <span className="chip-balance">{fmtAmount(user?.balance ?? '0')}</span>
            <span className="chip-account">
              {shortAccount(accountId)}
              {mock && <em> · mock</em>}
            </span>
          </button>
        ) : (
          <button type="button" className="btn btn-act" onClick={connect} disabled={connecting}>
            {connecting ? 'Opening…' : 'Connect wallet'}
          </button>
        )}
      </div>
    </header>
  );
}
