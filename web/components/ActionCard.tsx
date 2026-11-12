'use client';

import { useEffect, useRef, useState } from 'react';
import { useDrizzle } from '../lib/useDrizzle';
import { cityName } from '../lib/cities';
import { useNow } from '../lib/useNow';
import {
  ONE_DRIZZLE,
  ROUND_CLOSING_MESSAGE,
  isRoundClosing,
  minPlantAmount,
} from '../lib/round';
import { fmt, fmtAmount, fmtMm, isPositive, parseAmount, toBig, type U128 } from '../lib/types';
import { flyPlant } from '../lib/fx';
import { ChevronIcon, SeedIcon } from './Sprites';

const CHIPS = ['10', '25', '50'];

export function ActionCard() {
  const {
    accountId,
    user,
    connect,
    connecting,
    claimSeed,
    cities,
    selected,
    setSelected,
    config,
    round,
    roundAt,
    storageBounds,
    plant,
    uproot,
    rainDance,
    busy,
    error,
  } = useDrizzle();

  const [amount, setAmount] = useState('');
  const [formError, setFormError] = useState<string | null>(null);
  const [open, setOpen] = useState(true);
  const [danceOpen, setDanceOpen] = useState(false);
  const [danceAmount, setDanceAmount] = useState('1');

  const plantBtn = useRef<HTMLButtonElement>(null);
  const errorRef = useRef<string | null>(null);
  const now = useNow(1000);

  useEffect(() => {
    errorRef.current = error;
  }, [error]);

  useEffect(() => {
    setAmount('');
    setFormError(null);
    setDanceOpen(false);
  }, [selected, accountId]);

  const city = selected === null ? null : (cities[selected] ?? null);
  const mine = city ? (user?.cities.find((entry) => entry.index === city.index) ?? null) : null;
  const symbol = config?.symbol ?? 'DRIZZLE';
  const seeded = user?.seeded ?? false;
  const connected = accountId !== null;
  const balance = toBig(user?.balance ?? '0');
  const serverNow = round ? round.now + (now - roundAt) / 1000 : 0;
  const closing = isRoundClosing(config, round, serverNow);
  const min = city && config ? minPlantAmount(city, config.emission_per_round) : ONE_DRIZZLE;
  const typed = isPositive(amount) ? toBig(parseAmount(amount)) : 0n;
  const belowMin = typed > 0n && typed < min;
  const overBalance = typed > balance;
  const hasPosition =
    mine !== null && (toBig(mine.value) > 0n || toBig(mine.shares) > 0n || toBig(mine.pending) > 0n);
  const evapPct = ((config?.evap_bps ?? 10) / 100).toFixed(2);
  const storageNear = storageBounds ? fmt(storageBounds.min, 24, 3) : null;

  const surprise = () => {
    if (!cities.length) return;
    const last = round ? round.next_round - 1 : null;
    const justRained = cities.filter(
      (entry) => last !== null && entry.last_rain_round === last && entry.last_mm_tenths > 0,
    );
    const pool = justRained.length > 0 ? justRained : cities;
    const pick = pool.reduce((best, entry) => {
      if (justRained.length > 0) return entry.last_mm_tenths > best.last_mm_tenths ? entry : best;
      if (entry.last_mm_tenths !== best.last_mm_tenths) {
        return entry.last_mm_tenths > best.last_mm_tenths ? entry : best;
      }
      return entry.planters > best.planters ? entry : best;
    });
    setSelected(pick.index);
  };

  const maxString = (() => {
    const whole = balance / ONE_DRIZZLE;
    const frac = balance % ONE_DRIZZLE;
    return frac === 0n
      ? whole.toString()
      : `${whole}.${frac.toString().padStart(18, '0').replace(/0+$/, '')}`;
  })();

  const setChip = (chip: string) => {
    setFormError(null);
    setAmount(chip === 'Max' ? maxString : chip);
  };

  const onPlant = async () => {
    if (!city) return;
    try {
      if (!isPositive(amount)) throw new Error('type an amount first');
      const yocto = parseAmount(amount);
      if (BigInt(yocto) < min) {
        throw new Error(`plant at least ${fmtAmount(min.toString())} ${symbol} in this city`);
      }
      if (BigInt(yocto) > balance) throw new Error('that is more than your balance');
      setFormError(null);
      await plant(city.index, yocto);
      if (errorRef.current) return;
      flyPlant(city.index, plantBtn.current);
    } catch (err) {
      setFormError(err instanceof Error ? err.message : String(err));
    }
  };

  const onDance = async () => {
    if (!city) return;
    try {
      if (!isPositive(danceAmount)) throw new Error('type an amount first');
      const yocto: U128 = parseAmount(danceAmount);
      if (BigInt(yocto) > balance) throw new Error('that is more than your balance');
      setFormError(null);
      await rainDance(city.index, yocto);
      setDanceOpen(false);
    } catch (err) {
      setFormError(err instanceof Error ? err.message : String(err));
    }
  };

  let step: 1 | 2 | 3 | 4 = 4;
  if (!connected) step = 1;
  else if (!seeded) step = 2;
  else if (!city) step = 3;

  const summary =
    step === 1
      ? 'Grow coins with the weather'
      : step === 2
        ? 'Claim your 100 seed coins'
        : step === 3
          ? 'Pick a plot'
          : cityName(city!.index, city!.name);

  let plantReason: string | undefined;
  if (busy !== null) plantReason = 'one transaction at a time';
  else if (typed === 0n) plantReason = 'type an amount';
  else if (belowMin) plantReason = `minimum ${fmtAmount(min.toString())} ${symbol}`;
  else if (overBalance) plantReason = 'more than your balance';
  else if (closing) plantReason = ROUND_CLOSING_MESSAGE;
  else if (!hasPosition && typed === 0n) plantReason = 'type an amount';

  const plantLabel =
    busy === 'Plant' ? 'Planting…' : typed > 0n ? `Plant ${fmt(typed, 18, 2)} seeds` : 'Plant seeds';

  return (
    <aside className="action-card" data-open={open ? 'true' : 'false'} aria-label="Your next step">
      <button
        type="button"
        className="sheet-handle"
        onClick={() => setOpen((value) => !value)}
        aria-expanded={open}
      >
        <span className="handle-bar" aria-hidden />
        <span className="handle-label">{summary}</span>
        <ChevronIcon className={`handle-chevron${open ? ' is-open' : ''}`} />
      </button>

      <div className="action-body">
        {step === 1 && (
          <>
            <h2 className="action-title">Grow coins with the weather</h2>
            <p className="action-note">
              Plant $DRIZZLE in a city. When it rains there, you harvest new coins. Dry hours evaporate{' '}
              {evapPct}%.
            </p>
            <button type="button" className="btn btn-act btn-wide" onClick={connect} disabled={connecting}>
              {connecting ? 'Opening wallet…' : 'Connect wallet'}
            </button>
          </>
        )}

        {step === 2 && (
          <>
            <div className="action-plate">
              <span className="plate-icon">
                <SeedIcon className="plate-seed" />
              </span>
              <h2 className="action-title">Claim your 100 seed coins</h2>
            </div>
            <p className="action-note">
              One claim per account, just enough to plant in a few cities and see a round settle.
            </p>
            {storageNear && <p className="action-fine">≈{storageNear} NEAR refundable storage deposit</p>}
            <button
              type="button"
              className="btn btn-act btn-wide"
              onClick={claimSeed}
              disabled={busy !== null}
            >
              {busy === 'Claim seed' ? 'Claiming…' : 'Claim 100 seeds'}
            </button>
          </>
        )}

        {step === 3 && (
          <>
            <h2 className="action-title">Pick a plot</h2>
            <p className="action-note">Tap any city tile. Cities with rain clouds just got paid.</p>
            <button type="button" className="link-btn" onClick={surprise}>
              Surprise me
            </button>
          </>
        )}

        {step === 4 && city && (
          <>
            <div className="action-head">
              <h2 className="action-title">{cityName(city.index, city.name)}</h2>
              <button
                type="button"
                className="link-btn small"
                onClick={() => setSelected(null)}
                title="Clear the selection"
              >
                change
              </button>
            </div>

            <dl className="facts">
              <div>
                <dt>Staked</dt>
                <dd>{fmtAmount(city.value)}</dd>
              </div>
              <div>
                <dt>Planters</dt>
                <dd>{city.planters}</dd>
              </div>
              <div>
                <dt>Last rain</dt>
                <dd>{city.last_rain_round === null ? 'never' : `${fmtMm(city.last_mm_tenths)} mm`}</dd>
              </div>
            </dl>

            {mine && (toBig(mine.value) > 0n || toBig(mine.pending) > 0n) && (
              <div className="position">
                {toBig(mine.value) > 0n && (
                  <div className="position-row">
                    <span>Your stake</span>
                    <b>
                      {fmt(mine.value, 18, 1)} <small>{symbol}</small>
                    </b>
                  </div>
                )}
                {toBig(mine.pending) > 0n && (
                  <div className="position-row is-pending">
                    <span>Sprouting</span>
                    <b>
                      {fmt(mine.pending, 18, 1)}
                      {mine.pending_round !== null && (
                        <small> · sprouts after round {mine.pending_round}</small>
                      )}
                    </b>
                  </div>
                )}
              </div>
            )}

            <label className="amount-field">
              <span className="field-label">Amount</span>
              <span className="amount-input">
                <input
                  type="text"
                  inputMode="decimal"
                  placeholder="25"
                  value={amount}
                  onChange={(event) => {
                    setAmount(event.target.value);
                    setFormError(null);
                  }}
                  aria-label={`Amount in ${symbol}`}
                />
                <span className="amount-suffix">{symbol}</span>
              </span>
            </label>

            <div className="chips">
              {CHIPS.map((chip) => (
                <button
                  key={chip}
                  type="button"
                  className={`chip${amount === chip ? ' is-on' : ''}`}
                  onClick={() => setChip(chip)}
                >
                  {chip}
                </button>
              ))}
              <button
                type="button"
                className={`chip${amount === maxString && balance > 0n ? ' is-on' : ''}`}
                onClick={() => setChip('Max')}
              >
                Max
              </button>
            </div>

            <p className="amount-hint">
              min {fmtAmount(min.toString())} {symbol}
              {closing && <span className="hint-warn"> · {ROUND_CLOSING_MESSAGE}</span>}
            </p>

            {formError && <p className="form-error">{formError}</p>}
            {!formError && plantReason && typed > 0n && busy === null && (
              <p className="form-error is-soft">{plantReason}</p>
            )}

            <button
              type="button"
              ref={plantBtn}
              className="btn btn-act btn-wide"
              onClick={onPlant}
              disabled={busy !== null || closing || typed === 0n || belowMin || overBalance}
              title={plantReason}
            >
              {plantLabel}
            </button>

            <p className="action-fine">
              Coins sprout after the next settlement, so you earn from later rounds — not this one. Dry hours
              evaporate {evapPct}%.
            </p>

            <div className="secondary-actions">
              {hasPosition && (
                <button
                  type="button"
                  className="link-btn"
                  disabled={busy !== null || closing}
                  title={closing ? ROUND_CLOSING_MESSAGE : 'Withdraw everything in this city'}
                  onClick={() => uproot(city.index, null)}
                >
                  {busy === 'Uproot' ? 'Uprooting…' : 'Uproot all'}
                </button>
              )}
              <button
                type="button"
                className="link-btn"
                disabled={busy !== null}
                onClick={() => setDanceOpen((value) => !value)}
                aria-expanded={danceOpen}
              >
                Rain dance
              </button>
            </div>

            {danceOpen && (
              <div className="dance">
                <p className="dance-note">
                  Burn coins into this city&apos;s jackpot. It pays out the next time it rains here.
                </p>
                <div className="dance-row">
                  <span className="amount-input is-compact">
                    <input
                      type="text"
                      inputMode="decimal"
                      value={danceAmount}
                      onChange={(event) => setDanceAmount(event.target.value)}
                      aria-label="Rain dance amount"
                    />
                    <span className="amount-suffix">{symbol}</span>
                  </span>
                  <button
                    type="button"
                    className="btn btn-act"
                    onClick={onDance}
                    disabled={busy !== null || !isPositive(danceAmount)}
                  >
                    {busy === 'Rain dance' ? 'Dancing…' : `Burn ${danceAmount || '0'}`}
                  </button>
                </div>
              </div>
            )}

          </>
        )}
      </div>
    </aside>
  );
}
