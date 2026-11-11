'use client';

import { useEffect, useState } from 'react';
import { useDrizzle } from '../lib/useDrizzle';
import { fetchHourlyReadings, type HourlyReading } from '../lib/openmeteo';
import { fmtHour, fmtHourIso, fmtMm } from '../lib/types';
import { attestationUrl } from '../lib/outlayer';

type VerifyState =
  | { status: 'loading' }
  | { status: 'ok'; url: string; readings: HourlyReading[] }
  | { status: 'error'; message: string };

export function VerifyDrawer() {
  const { verify, closeVerify, config } = useDrizzle();
  const [state, setState] = useState<VerifyState>({ status: 'loading' });

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
    <div className="drawer-backdrop" onClick={closeVerify} role="presentation">
      <aside
        className="drawer"
        role="dialog"
        aria-modal="true"
        aria-label={`Verify round ${verify.round}`}
        onClick={(event) => event.stopPropagation()}
      >
        <header className="drawer-head">
          <div>
            <p className="eyebrow">verify</p>
            <h2>Round {verify.round}</h2>
            <p className="muted tiny">
              {fmtHour(verify.hour * 3600)} · hour {hourIso}
            </p>
          </div>
          <button className="btn btn-ghost" onClick={closeVerify} aria-label="Close verify drawer">
            ✕
          </button>
        </header>

        {state.status === 'loading' && <p className="muted">Re-fetching that hour from Open-Meteo…</p>}
        {state.status === 'error' && (
          <p className="form-error">
            Open-Meteo re-fetch failed: {state.message}. The on-chain readings below still stand; Open-Meteo may
            also have revised old hours.
          </p>
        )}

        {state.status === 'ok' && (
          <div className={`verify-banner ${allMatch ? 'is-ok' : 'is-warn'}`}>
            {allMatch ? (
              <>✅ all 12 match — the contract stored exactly what Open-Meteo returns for this hour</>
            ) : (
              <>
                ⚠️ {mismatches} of 12 differ — Open-Meteo sometimes revises an hour after settlement, and the
                oracle's reading was attested inside OutLayer at settlement time. Check the attestation below.
              </>
            )}
          </div>
        )}

        <table className="verify-table">
          <thead>
            <tr>
              <th>city</th>
              <th>on-chain</th>
              <th>Open-Meteo</th>
              <th aria-label="match" />
            </tr>
          </thead>
          <tbody>
            {config?.cities.map((city, index) => {
              const onChain = verify.mm_tenths[index] ?? 0;
              const reading = state.status === 'ok' ? state.readings[index] : null;
              const match = reading ? reading.tenths === onChain : null;
              return (
                <tr key={city.name}>
                  <td>{city.name}</td>
                  <td className="mono">{fmtMm(onChain)} mm</td>
                  <td className="mono">
                    {reading ? `${reading.mm === null ? 'null → ' : ''}${fmtMm(reading.tenths)} mm` : '…'}
                  </td>
                  <td className={match === null ? '' : match ? 'is-match' : 'is-mismatch'}>
                    {match === null ? '' : match ? '✓' : '✗'}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>

        {state.status === 'ok' && (
          <p className="muted tiny break">
            same URL the oracle used: <code>{state.url}</code>
          </p>
        )}

        <div className="attest">
          <h3>OutLayer attestation</h3>
          {verify.outlayer_request_id ? (
            <p className="tiny break">
              request <code>{verify.outlayer_request_id}</code>
              <br />
              <a href={attestationUrl(verify.outlayer_request_id)} target="_blank" rel="noreferrer">
                open attestation ↗
              </a>
            </p>
          ) : (
            <p className="muted tiny">n/a — this round has no request id</p>
          )}
        </div>

        <div className="attest">
          <h3>Pinned code source</h3>
          <pre className="code-block">{source ? JSON.stringify(source, null, 2) : 'unknown'}</pre>
          <p className="muted tiny">
            The contract pins this by WASM hash. OutLayer attests that exactly this binary ran inside an Intel TDX
            enclave, and only the enclave callback can apply the readings.
          </p>
        </div>
      </aside>
    </div>
  );
}
