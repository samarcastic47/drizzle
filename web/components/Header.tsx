'use client';

import { useDrizzle } from '../lib/useDrizzle';
import { fmtAmount, shortAccount } from '../lib/types';

export function Header() {
  const { accountId, user, mock, mode, connect, disconnect, connecting, claimSeed, busy, config } = useDrizzle();
  const seeded = user?.seeded ?? false;

  return (
    <header className="site-header">
      <div className="brand">
        <span className="brand-mark" aria-hidden>
          ☔
        </span>
        <div>
          <h1>
            Drizzle <span className="ticker">$DRIZZLE</span>
          </h1>
          <p className="brand-sub">The memecoin printed by the sky</p>
        </div>
      </div>

      <div className="header-meta">
        <span className={`badge badge-${mode}`}>{mode === 'replay' ? 'replay' : 'live'}</span>
        <span className="badge badge-net">{mock ? 'mock data' : 'NEAR testnet'}</span>
      </div>

      <div className="wallet">
        {accountId ? (
          <>
            <div className="wallet-balance" title="Your $DRIZZLE balance">
              <span>{fmtAmount(user?.balance ?? '0')}</span>
              <small>{config?.symbol ?? 'DRIZZLE'}</small>
            </div>
            {!seeded && (
              <button className="btn btn-primary" onClick={claimSeed} disabled={busy !== null}>
                {busy === 'Claim seed' ? 'Claiming…' : 'Claim seed'}
              </button>
            )}
            <button
              className="btn btn-ghost wallet-id"
              onClick={disconnect}
              aria-label="Disconnect wallet"
              title={mock ? 'Leave the mock account' : 'Disconnect wallet'}
            >
              {shortAccount(accountId)}
              {mock && <em> · mock</em>}
            </button>
          </>
        ) : (
          <button className="btn btn-primary" onClick={connect} disabled={connecting}>
            {connecting ? 'Opening…' : 'Connect wallet'}
          </button>
        )}
      </div>
    </header>
  );
}
