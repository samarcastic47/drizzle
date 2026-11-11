/** OutLayer job lookups and attestation links. */

const JOBS_API = 'https://testnet-api.outlayer.ai/public/jobs';

/** OutLayer's jobs explorer, used whenever the job id cannot be resolved in the browser. */
export const OUTLAYER_JOBS_URL = 'https://app.outlayer.ai/executions';

export interface OutlayerJob {
  jobId: number;
  txHash: string | null;
}

/** `/attestation/{id}` takes OutLayer's job_id, not the on-chain request_id. */
export function attestationUrl(jobId: number): string {
  return `https://app.outlayer.ai/attestation/${jobId}?network=testnet`;
}

export function nearblocksTxUrl(hash: string): string {
  return `https://testnet.nearblocks.io/txns/${hash}`;
}

export function nearblocksBlockUrl(height: number): string {
  return `https://testnet.nearblocks.io/block/${height}`;
}

interface JobRow {
  job_id?: unknown;
  request_id?: unknown;
  job_type?: unknown;
  transaction_hash?: unknown;
}

/**
 * Resolve the OutLayer job for a round's request id.
 *
 * `testnet-api.outlayer.ai` only sends `access-control-allow-origin` for
 * `https://app.outlayer.ai`, so a browser fetch from any other origin is blocked
 * by CORS. That is not an error here: the drawer falls back to the jobs explorer.
 */
export async function findJob(requestId: string, keeper: string): Promise<OutlayerJob | null> {
  const wanted = Number(requestId);
  if (!Number.isFinite(wanted)) return null;
  const url = `${JOBS_API}?user_account_id=${encodeURIComponent(keeper)}&limit=100`;

  let rows: unknown;
  try {
    const res = await fetch(url, { headers: { accept: 'application/json' } });
    if (!res.ok) return null;
    rows = await res.json();
  } catch {
    return null;
  }
  if (!Array.isArray(rows)) return null;

  const match = (rows as JobRow[]).find(
    (row) => Number(row.request_id) === wanted && row.job_type === 'execute',
  );
  if (!match || typeof match.job_id !== 'number') return null;
  return {
    jobId: match.job_id,
    txHash: typeof match.transaction_hash === 'string' ? match.transaction_hash : null,
  };
}
