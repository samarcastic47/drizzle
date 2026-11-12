'use client';

import { useEffect, useState } from 'react';
import { useDrizzle } from '../lib/useDrizzle';
import { fetchHourlyReadings, type HourlyReading } from '../lib/openmeteo';
import { fmtHour, fmtHourIso, fmtMm } from '../lib/types';
import { cityName } from '../lib/cities';
import {
  OUTLAYER_JOBS_URL,
  attestationUrl,
  findJob,
  nearblocksBlockUrl,
  nearblocksTxUrl,
  type OutlayerJob,
} from '../lib/outlayer';
import { CloseIcon } from './Sprites';

type VerifyState =
  | { status: 'loading' }
  | { status: 'ok'; url: string; readings: HourlyReading[] }
  | { status: 'error'; message: string };

type JobState =
  | { status: 'none' }
  | { status: 'loading' }
  | { status: 'found'; job: OutlayerJob }
  | { status: 'unresolved' };

export function VerifyDrawer() {
  const { verify, closeVerify, config } = useDrizzle();
  const [state, setState] = useState<VerifyState>({ status: 'loading' });
  const [jobState, setJobState] = useState<JobState>({ status: 'none' });

  useEffect(() => {
    if (!verify || !config) return;
    const controller = new AbortController();
    const hourIso = fmtHourIso(verify.hour * 3600);
    setState({ status: 'loading' });
    fetchHourlyReadings(config.cities, hourIso, controller.signal)
      .then((result) => setState({ status: 'ok', url: result.url, readings: result.readings }))
      .catch((err: unknown) => {
        if (controller.signal.aborted) return;
        setState({ status: 'error', message: err instanceof Error ? err.message : String(err) });
      });
    return () => controller.abort();
  }, [verify, config]);

  useEffect(() => {
    const requestId = verify?.outlayer_request_id ?? null;
    const keeper = verify?.keeper;
    if (!requestId || !keeper) {
      setJobState({ status: 'none' });
      return;
    }
    let cancelled = false;
    setJobState({ status: 'loading' });
    findJob(requestId, keeper)
      .then((job) => {
        if (cancelled) return;
        setJobState(job ? { status: 'found', job } : { status: 'unresolved' });
      })
      .catch(() => {
        if (!cancelled) setJobState({ status: 'unresolved' });
      });
    return () => {
      cancelled = true;
    };
  }, [verify]);

  useEffect(() => {
    if (!verify) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') closeVerify();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [verify, closeVerify]);

  if (!verify) return null;

  const hourIso = fmtHourIso(verify.hour * 3600);
  const mismatches =
    state.status === 'ok'
      ? state.readings.filter((reading, index) => reading.tenths !== (verify.mm_tenths[index] ?? 0)).length
      : 0;
  const allMatch = state.status === 'ok' && mismatches === 0;
  const source = config?.oracle.source;

  return (
    <div className="sheet-backdrop" onClick={closeVerify} role="presentation">
      <aside
        className="sheet verify-sheet"
        role="dialog"
        aria-modal="true"
        aria-label={`Verify round ${verify.round}`}
        onClick={(event) => event.stopPropagation()}
      >
        <header className="sheet-head">
          <div>
            <h2 className="sheet-title">Round {verify.round}</h2>
            <p className="sheet-note small">
              {fmtHour(verify.hour * 3600)} · hour {hourIso} ·{' '}
              <a href={nearblocksBlockUrl(verify.tick_block_height)} target="_blank" rel="noreferrer">
                tick block {verify.tick_block_height.toLocaleString('en-US')}
              </a>
            </p>
          </div>
          <button type="button" className="icon-btn" onClick={closeVerify} aria-label="Close verify panel">
            <CloseIcon />
          </button>
        </header>

        <div className="sheet-body">
          {state.status === 'loading' && <p className="empty-line">Re-fetching that hour from Open-Meteo…</p>}
          {state.status === 'error' && (
            <p className="form-error">
              Open-Meteo re-fetch failed: {state.message}. The on-chain readings still stand; Open-Meteo can also
              revise an old hour.
            </p>
          )}

          {state.status === 'ok' && (
            <div className={`verify-banner ${allMatch ? 'is-ok' : 'is-warn'}`}>
              {allMatch
                ? 'All 12 match — the contract stored exactly what Open-Meteo returns for this hour.'
                : `${mismatches} of 12 differ — Open-Meteo sometimes revises an hour after settlement; the oracle read it inside OutLayer at settlement time. Check the attestation below.`}
            </div>
          )}

          <table className="verify-table">
            <thead>
              <tr>
                <th>City</th>
                <th>On-chain</th>
                <th>Open-Meteo</th>
                <th className="match-col">Check</th>
              </tr>
            </thead>
            <tbody>
              {config?.cities.map((city, index) => {
                const onChain = verify.mm_tenths[index] ?? 0;
                const reading = state.status === 'ok' ? state.readings[index] : null;
                const match = reading ? reading.tenths === onChain : null;
                return (
                  <tr key={city.name}>
                    <td>{cityName(index, city.name)}</td>
                    <td className="num">{fmtMm(onChain)} mm</td>
                    <td className="num">
                      {reading ? `${reading.mm === null ? 'null → ' : ''}${fmtMm(reading.tenths)} mm` : '…'}
                    </td>
                    <td className={match === null ? '' : match ? 'is-match' : 'is-mismatch'}>
                      {match === null ? '' : match ? 'match' : 'differs'}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>

          {state.status === 'ok' && (
            <p className="sheet-note small break">
              Same URL the oracle used: <code>{state.url}</code>
            </p>
          )}

          <section className="attest">
            <h3>OutLayer attestation</h3>
            {!verify.outlayer_request_id ? (
              <p className="sheet-note small">n/a — this round has no request id</p>
            ) : (
              <>
                <p className="sheet-note small break">
                  request <code>{verify.outlayer_request_id}</code> · keeper <code>{verify.keeper}</code>
                </p>
                {jobState.status === 'loading' && <p className="empty-line">Resolving the OutLayer job…</p>}
                {jobState.status === 'found' && (
                  <>
                    <p className="sheet-note small">
                      job <code>{jobState.job.jobId}</code>
                    </p>
                    <p className="sheet-note small">
                      <a href={attestationUrl(jobState.job.jobId)} target="_blank" rel="noreferrer">
                        open attestation ↗
                      </a>
                      {jobState.job.txHash && (
                        <>
                          {' · '}
                          <a href={nearblocksTxUrl(jobState.job.txHash)} target="_blank" rel="noreferrer">
                            request tx ↗
                          </a>
                        </>
                      )}
                    </p>
                  </>
                )}
                {jobState.status === 'unresolved' && (
                  <>
                    <p className="sheet-note small">
                      <a href={OUTLAYER_JOBS_URL} target="_blank" rel="noreferrer">
                        OutLayer jobs explorer ↗
                      </a>
                    </p>
                    <p className="sheet-note small">
                      The job id cannot be resolved from this page: testnet-api.outlayer.ai only allows the origin
                      app.outlayer.ai, so the browser blocks the lookup. The request id, tick block and pinned
                      code hash are still checkable on-chain.
                    </p>
                  </>
                )}
              </>
            )}
          </section>

          <section className="attest">
            <h3>Pinned code source</h3>
            <pre className="code-block">{source ? JSON.stringify(source, null, 2) : 'unknown'}</pre>
            <p className="sheet-note small">
              The contract pins this by WASM hash. OutLayer attests that exactly this binary ran inside an Intel
              TDX enclave, and only the enclave callback can apply the readings.
            </p>
          </section>
        </div>
      </aside>
    </div>
  );
}
