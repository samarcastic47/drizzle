/** Minimal NEAR JSON-RPC client for read-only calls (no near-api-js). */

import type { AccessKeyInfo, U128 } from './types';

export const RPC_URL = process.env.NEXT_PUBLIC_RPC_URL || 'https://rpc.testnet.near.org';

function decodeResult(result: unknown): string {
  if (typeof result === 'string') {
    // Older nodes return base64 bytes; newer ones return a byte array.
    try {
      return new TextDecoder().decode(Uint8Array.from(atob(result), (c) => c.charCodeAt(0)));
    } catch {
      return result;
    }
  }
  if (Array.isArray(result)) return new TextDecoder().decode(Uint8Array.from(result as number[]));
  if (result && typeof result === 'object' && 'result' in (result as Record<string, unknown>)) {
    return decodeResult((result as Record<string, unknown>).result);
  }
  throw new Error('unexpected RPC result shape');
}

export class RpcError extends Error {
  constructor(
    message: string,
    readonly code?: number,
  ) {
    super(message);
    this.name = 'RpcError';
  }
}

export async function rpc<T>(method: string, params: unknown): Promise<T> {
  let res: Response;
  try {
    res = await fetch(RPC_URL, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ jsonrpc: '2.0', id: 'drizzle', method, params }),
    });
  } catch (err) {
    throw new RpcError(`RPC unreachable: ${(err as Error).message}`);
  }
  const json = (await res.json().catch(() => null)) as
    | { result?: T; error?: { message?: string; cause?: { name?: string }; code?: number } }
    | null;
  if (!json) throw new RpcError(`RPC returned non-JSON (${res.status})`);
  if (json.error) {
    const cause = json.error.cause?.name;
    const msg = json.error.message || 'RPC error';
    throw new RpcError(cause ? `${msg} (${cause})` : msg, json.error.code);
  }
  if (json.result === undefined) throw new RpcError('RPC returned no result');
  return json.result;
}

/**
 * Nodes differ: newer RPCs expose `call_function`, older/2026 testnet nodes only
 * accept the legacy `query` request with `request_type: "call_function"`.
 * Probe once, then stick to the style that works.
 */
let viewStyle: 'call_function' | 'query' | null = null;

/** The legacy `query` style reports contract failures as `result.error`. */
function unwrapViewResult(res: { result?: unknown; error?: unknown }): unknown {
  if (typeof res.error === 'string' && res.error) throw new RpcError(res.error);
  return res.result;
}

async function rawViewResult(params: {
  finality: 'final' | 'optimistic';
  account_id: string;
  method_name: string;
  args_base64: string;
}): Promise<unknown> {
  if (viewStyle !== 'query') {
    try {
      const res = await rpc<{ result: unknown; error?: unknown }>('call_function', params);
      viewStyle = 'call_function';
      return unwrapViewResult(res);
    } catch (err) {
      const unsupported = err instanceof RpcError && /METHOD_NOT_FOUND|Method not found/i.test(err.message);
      if (viewStyle === 'call_function' || !unsupported) throw err;
    }
  }
  const res = await rpc<{ result: unknown; error?: unknown }>('query', {
    request_type: 'call_function',
    ...params,
  });
  viewStyle = 'query';
  return unwrapViewResult(res);
}

/** call a view method and JSON-parse its bytes */
export async function view<T>(
  contractId: string,
  methodName: string,
  args: Record<string, unknown> = {},
  finality: 'final' | 'optimistic' = 'final',
): Promise<T> {
  const result = await rawViewResult({
    finality,
    account_id: contractId,
    method_name: methodName,
    args_base64: base64Json(args),
  });
  const text = decodeResult(result);
  if (!text) return null as T;
  try {
    return JSON.parse(text) as T;
  } catch {
    // Methods that return a bare string still land here as invalid JSON.
    return text as unknown as T;
  }
}

export function base64Json(value: unknown): string {
  const bytes = new TextEncoder().encode(JSON.stringify(value));
  let binary = '';
  for (const b of bytes) binary += String.fromCharCode(b);
  return btoa(binary);
}

export async function ftBalanceOf(contractId: string, accountId: string): Promise<U128> {
  const raw = await view<string>(contractId, 'ft_balance_of', { account_id: accountId });
  return typeof raw === 'string' && /^\d+$/.test(raw) ? raw : '0';
}

/** `near account list-keys` equivalent, over RPC. */
export async function viewAccessKeyList(accountId: string): Promise<AccessKeyInfo[]> {
  const res = await rpc<{ keys: AccessKeyInfo[] }>('query', {
    request_type: 'view_access_key_list',
    finality: 'final',
    account_id: accountId,
  });
  return res.keys ?? [];
}

/** Whether an account (or contract) exists at all on chain. */
export async function accountExists(accountId: string): Promise<boolean> {
  try {
    await rpc('query', { request_type: 'view_account', finality: 'final', account_id: accountId });
    return true;
  } catch {
    return false;
  }
}
