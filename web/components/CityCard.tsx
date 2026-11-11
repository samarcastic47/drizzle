'use client';

import { useEffect, useState } from 'react';
import { useDrizzle } from '../lib/useDrizzle';
import { fmtAmount, fmtMm, isPositive, parseAmount } from '../lib/types';

export function CityCard() {
  const { cities, selected, user, accountId, plant, uproot, rainDance, busy, config } = useDrizzle();
  const [amount, setAmount] = useState('');
  const [formError, setFormError] = useState<string | null>(null);

  useEffect(() => {
    setAmount('');
    setFormError(null);
  }, [selected]);

  const city = selected === null ? null : cities[selected];
  const mine = city ? user?.cities[city.index] : null;
  const connected = accountId !== null;
  const disabledReason = connected ? undefined : 'Connect a wallet first';
  const symbol = config?.symbol ?? 'DRIZZLE';

  const withAmount = (fn: (yocto: string) => void) => {
    try {
      if (!isPositive(amount)) throw new Error('enter an amount');
      setFormError(null);
      fn(parseAmount(amount));
    } catch (err) {
      setFormError(err instanceof Error ? err.message : String(err));
    }
  };

  if (!city) {
    return (
      <section className="card city-card city-card-empty">
        <h2>Pick a city</h2>
        <p className="muted">
          Tap a pin on the map — or a city in the list — to see its crowd, its jackpot and its last rain.
        </p>
        <p className="muted small">
          Wet, empty cities pay the most. Crowded wet cities split the same rain between more planters.
        </p>
      </section>
    );
  }

  return (
    <section className="card city-card">
      <header className="city-head">
        <div>
          <p className="eyebrow">selected city</p>
          <h2>{city.name}</h2>
        </div>
        <span className={`rain-badge ${city.last_rain_round !== null ? 'is-wet' : ''}`}>
          {city.last_rain_round === null ? 'never rained' : `last rain round ${city.last_rain_round}`}
        </span>
      </header>

      <dl className="stat-grid">
        <div>
          <dt>planters</dt>
          <dd>{city.planters}</dd>
        </div>
        <div>
          <dt>staked</dt>
          <dd>
            {fmtAmount(city.value)} <small>{symbol}</small>
          </dd>
        </div>
        <div>
          <dt>jackpot</dt>
          <dd>
            {fmtAmount(city.jackpot)} <small>{symbol}</small>
          </dd>
        </div>
        <div>
          <dt>last rain</dt>
          <dd>{city.last_rain_round === null ? 'never' : `${fmtMm(city.last_mm_tenths)} mm`}</dd>
        </div>
      </dl>

      <div className="you">
        <h3>Your position</h3>
        {connected ? (
          <dl className="stat-grid small-grid">
            <div>
              <dt>stake value</dt>
              <dd>
                {fmtAmount(mine?.value ?? '0')} <small>{symbol}</small>
              </dd>
            </div>
            <div>
              <dt>sprouting</dt>
              <dd>
                {fmtAmount(mine?.pending ?? '0')}
                {mine && mine.pending_round !== null && (
                  <small> · after round {mine.pending_round}</small>
                )}
              </dd>
            </div>
          </dl>
        ) : (
          <p className="muted small">Connect a wallet to plant, uproot or rain dance.</p>
        )}
      </div>

      <label className="amount-field">
        <span>amount</span>
        <input
          type="text"
          inputMode="decimal"
          placeholder="50"
          value={amount}
          onChange={(event) => {
            setAmount(event.target.value);
            setFormError(null);
          }}
          aria-label={`Amount in ${symbol}`}
        />
        <span className="amount-suffix">{symbol}</span>
      </label>
      {formError && <p className="form-error">{formError}</p>}

      <div className="action-row">
        <button
          className="btn btn-primary"
          disabled={!connected || busy !== null}
          title={disabledReason}
          onClick={() => withAmount((yocto) => plant(city.index, yocto))}
        >
          {busy === 'Plant' ? 'Planting…' : 'Plant'}
        </button>
        <button
          className="btn"
          disabled={!connected || busy !== null || (mine?.shares ?? '0') === '0'}
          title={disabledReason ?? 'Withdraw your whole position in this city'}
          onClick={() => uproot(city.index, null)}
        >
          {busy === 'Uproot' ? 'Uprooting…' : 'Uproot all'}
        </button>
        <button
          className="btn btn-rain"
          disabled={!connected || busy !== null}
          title={disabledReason ?? 'Burn $DRIZZLE into the cloud jackpot; it pays this city the next time it rains'}
          onClick={() => withAmount((yocto) => rainDance(city.index, yocto))}
        >
          {busy === 'Rain dance' ? 'Dancing…' : 'Rain dance'}
        </button>
      </div>

      <p className="muted tiny">
        Planted coins sprout after the next settlement, so you cannot earn the round you planted in. Dry rounds
        evaporate {config ? (config.evap_bps / 100).toFixed(2) : '0.10'}% of the pool.
      </p>
    </section>
  );
}
