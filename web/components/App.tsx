'use client';

import { Header } from './Header';
import { RoundBar } from './RoundBar';
import { WorldMap, CityChips } from './WorldMap';
import { CityCard } from './CityCard';
import { Feed } from './Feed';
import { About } from './About';
import { VerifyDrawer } from './VerifyDrawer';
import { useDrizzle } from '../lib/useDrizzle';

export function App() {
  const {
    booted,
    loadError,
    mode,
    setMode,
    error,
    notice,
    clearMessages,
    refresh,
    contractId,
    mock,
    replayContract,
    liveContract,
  } = useDrizzle();

  return (
    <div className="page">
      <Header />

      {(error || notice) && (
        <div className={`toast ${error ? 'is-error' : 'is-ok'}`} role="status">
          <span>{error ?? notice}</span>
          <button className="btn btn-tiny" onClick={clearMessages} aria-label="Dismiss message">
            ✕
          </button>
        </div>
      )}

      {booted && <RoundBar />}

      {!booted ? (
        <section className="card loading-card">
          <p>Loading round data…</p>
        </section>
      ) : loadError ? (
        <section className="card not-deployed">
          <h2>
            {loadError === 'not-deployed'
              ? mode === 'live'
                ? 'live instance not deployed yet'
                : 'replay instance not deployed yet'
              : 'could not read the contract'}
          </h2>
          {loadError === 'not-deployed' ? (
            <>
              <p className="muted">
                <code>{contractId}</code> has no Drizzle contract deployed on NEAR testnet yet. Nothing is lost:
                the replay instance settles a real past week of Open-Meteo weather, one round per minute, through
                the same contract and the same OutLayer TEE path.
              </p>
              {mode === 'live' ? (
                <button className="btn btn-primary" onClick={() => setMode('replay')}>
                  Open replay ({replayContract})
                </button>
              ) : (
                <button className="btn" onClick={() => setMode('live')}>
                  Try live ({liveContract})
                </button>
              )}
            </>
          ) : (
            <>
              <p className="form-error">{loadError}</p>
              <p className="muted tiny">
                The page is a static export; reads go straight from your browser to{' '}
                <code>{process.env.NEXT_PUBLIC_RPC_URL || 'https://rpc.testnet.near.org'}</code>.
              </p>
              <button className="btn" onClick={() => void refresh()}>
                Retry
              </button>
            </>
          )}
          {mock && (
            <p className="muted tiny">
              Mock mode (<code>NEXT_PUBLIC_MOCK=1</code>) serves in-memory data and never fails to deploy — this
              card only appears against real RPC.
            </p>
          )}
        </section>
      ) : (
        <>
          <section className="board">
            <div className="card map-card">
              <WorldMap />
              <CityChips />
            </div>
            <CityCard />
          </section>

          <section className="lower">
            <Feed />
          </section>

          <About />
        </>
      )}

      <VerifyDrawer />

      <footer className="site-footer">
        <p>
          Drizzle ☔ · NEP-141 on NEAR testnet · weather read inside an OutLayer Intel TDX enclave · the contract
          account holds no keys
        </p>
        <p className="muted tiny">
          data: Open-Meteo (CC-BY 4.0) · replay 1 round = 1 real hour · not financial advice, it is a rain
          simulator with a token attached
        </p>
      </footer>
    </div>
  );
}
