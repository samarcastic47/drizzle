'use client';

import { useEffect, useState } from 'react';
import { useDrizzle } from '../lib/useDrizzle';
import { fmtHour } from '../lib/types';
import { TopBar } from './TopBar';
import { Board } from './Board';
import { ActionCard } from './ActionCard';
import { Drawer, type DrawerTab } from './Drawer';
import { VerifyDrawer } from './VerifyDrawer';
import { CloseIcon } from './Sprites';

export function App() {
  const { booted, loadError, mode, setMode, error, notice, clearMessages, refresh, contractId, mock, replayContract, liveContract, round, config } =
    useDrizzle();
  const [drawer, setDrawer] = useState<DrawerTab | null>(null);

  useEffect(() => {
    if (!error && !notice) return;
    const timer = window.setTimeout(clearMessages, 8000);
    return () => window.clearTimeout(timer);
  }, [error, notice, clearMessages]);

  return (
    <div className="page">
      <TopBar onOpenRules={() => setDrawer('rounds')} />

      {(error || notice) && (
        <div className={`toast${error ? ' is-error' : ' is-ok'}`} role="status">
          <span>{error ?? notice}</span>
          <button type="button" className="icon-btn" onClick={clearMessages} aria-label="Dismiss message">
            <CloseIcon />
          </button>
        </div>
      )}

      <main className="stage" id="board">
        <div className="layout">
          <div className="board-col">
            {!booted ? (
              <div className="stage-loading">
                <p className="loading-line">Reading the round from NEAR…</p>
                <div className="board board-skeleton" aria-hidden>
                  {Array.from({ length: 12 }, (_, i) => (
                    <span key={i} className="plot-skeleton" />
                  ))}
                </div>
              </div>
            ) : loadError ? (
              <section className="stage-loading">
                <h2 className="action-title">
                  {loadError === 'not-deployed'
                    ? mode === 'live'
                      ? 'Live instance is not deployed yet'
                      : 'Replay instance is not deployed yet'
                    : 'Could not read the contract'}
                </h2>
                {loadError === 'not-deployed' ? (
                  <>
                    <p className="action-note">
                      <code>{contractId}</code> has no Drizzle contract on NEAR testnet yet. The replay instance
                      settles a real past week of Open-Meteo weather, one round per minute, through the same
                      contract and the same OutLayer enclave.
                    </p>
                    {mode === 'live' ? (
                      <button type="button" className="btn btn-act" onClick={() => setMode('replay')}>
                        Open replay ({replayContract})
                      </button>
                    ) : (
                      <button type="button" className="btn btn-act" onClick={() => setMode('live')}>
                        Try live ({liveContract})
                      </button>
                    )}
                  </>
                ) : (
                  <>
                    <p className="form-error">{loadError}</p>
                    <p className="action-fine">
                      Static export: reads go from your browser to{' '}
                      {process.env.NEXT_PUBLIC_RPC_URL || 'https://test.rpc.fastnear.com'}.
                    </p>
                    <button type="button" className="btn btn-act" onClick={() => void refresh()}>
                      Retry
                    </button>
                  </>
                )}
                {mock && <p className="action-fine">Mock mode serves in-memory data and never hits RPC.</p>}
              </section>
            ) : (
              <Board />
            )}
          </div>

          {booted && !loadError && <ActionCard />}
        </div>

        {round && config && (
          <p className="clock-line">
            {mode === 'replay' ? 'Replay' : 'Live'} · settling {fmtHour(round.next_hour * 3600)} · one round
            every {config.round_seconds}s
          </p>
        )}
      </main>

      <footer className="quiet-footer">
        Drizzle — the memecoin printed by the sky. NEAR testnet · weather by Open-Meteo (CC-BY 4.0) · not
        financial advice.
      </footer>

      <Drawer tab={drawer} onTab={setDrawer} onClose={() => setDrawer(null)} />
      <VerifyDrawer />
    </div>
  );
}
