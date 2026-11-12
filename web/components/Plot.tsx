'use client';

import type { CityInfo, UserCity } from '../lib/types';
import { fmt, fmtMm, toBig } from '../lib/types';
import { cityName } from '../lib/cities';
import { CoinIcon, Crop, DrySun, FlagIcon, RainCloud } from './Sprites';

export interface PlotFx {
  kind: 'rain' | 'dry';
  minted: string;
  seq: number;
}

export type CropStage = 0 | 1 | 2 | 3;

const DROPS = [
  { x: 18, delay: 0, dur: 780 },
  { x: 29, delay: 70, dur: 720 },
  { x: 40, delay: 150, dur: 820 },
  { x: 52, delay: 40, dur: 760 },
  { x: 63, delay: 120, dur: 700 },
  { x: 74, delay: 200, dur: 840 },
  { x: 34, delay: 260, dur: 740 },
  { x: 57, delay: 300, dur: 800 },
  { x: 46, delay: 360, dur: 720 },
  { x: 68, delay: 420, dur: 780 },
];

export function cropStage(staked: bigint): CropStage {
  if (staked <= 0n) return 0;
  const whole = Number(staked / 10n ** 18n);
  if (whole < 1_000) return 1;
  if (whole < 4_000) return 2;
  return 3;
}

interface Props {
  city: CityInfo;
  mine: UserCity | null;
  selected: boolean;
  changed: boolean;
  fx: PlotFx | undefined;
  evapPct: string;
  roundNumber: number | null;
  onSelect: (index: number) => void;
  onSeen: (index: number) => void;
}

export function Plot({ city, mine, selected, changed, fx, evapPct, roundNumber, onSelect, onSeen }: Props) {
  const total = toBig(city.value) + toBig(city.pending_total);
  const stage = cropStage(total);
  const wet =
    city.last_rain_round !== null &&
    roundNumber !== null &&
    city.last_rain_round === roundNumber - 1;
  const mm = city.last_mm_tenths;
  const hasStake = mine !== null && toBig(mine.value) > 0n;
  const hasPending = mine !== null && (toBig(mine.pending) > 0n || toBig(mine.shares) > 0n);
  const jackpot = toBig(city.jackpot);
  const symbol = 'DRIZZLE';
  const name = cityName(city.index, city.name);
  const minted = toBig(fx?.minted ?? '0');

  const label = [
    name,
    wet ? `last round ${fmtMm(mm)} mm of rain` : 'dry last round',
    `${fmt(city.value)} ${symbol} staked`,
    `${city.planters} planters`,
    jackpot > 0n ? `jackpot ${fmt(jackpot)}` : null,
    hasStake ? `you have ${fmt(mine!.value)} staked` : null,
    hasPending ? 'your coins are sprouting' : null,
  ]
    .filter(Boolean)
    .join(' · ');

  return (
    <button
      type="button"
      className={`plot${selected ? ' is-selected' : ''}${changed ? ' is-changed' : ''}`}
      data-plot={city.index}
      data-fx={fx?.kind}
      aria-pressed={selected}
      aria-label={label}
      onClick={() => {
        onSelect(city.index);
        onSeen(city.index);
      }}
      onMouseEnter={() => onSeen(city.index)}
      onFocus={() => onSeen(city.index)}
    >
      <span className="plot-weather">
        {wet ? (
          <>
            <RainCloud className="weather-art" />
            <span className="weather-text wet">{fmtMm(mm)} mm</span>
          </>
        ) : (
          <>
            <DrySun className="weather-art" />
            <span className="weather-text">{city.last_rain_round === null ? 'never rained' : 'dry'}</span>
          </>
        )}
      </span>

      <span className="plot-soil">
        <span className="soil-furrows" aria-hidden />
        {stage > 0 && <Crop stage={stage} className="crop" />}
        {hasStake && (
          <span className="plot-flag">
            <FlagIcon />
            yours: {fmt(mine!.value, 18, 1)}
          </span>
        )}
        {hasPending && <span className="plot-sprout">sprouting</span>}

        {fx?.kind === 'rain' && (
          <span className="plot-fx fx-rain" key={fx.seq} aria-hidden>
            <svg className="fx-cloud" viewBox="0 0 64 30" width="64" height="30">
              <g fill="var(--cloud)">
                <circle cx="22" cy="14" r="9" />
                <circle cx="34" cy="10" r="11" />
                <circle cx="46" cy="15" r="7.5" />
                <rect x="12" y="15" width="42" height="7" rx="3.5" />
              </g>
            </svg>
            {DROPS.map((d, i) => (
              <i
                key={i}
                className="fx-drop"
                style={{
                  left: `${d.x}%`,
                  animationDelay: `${d.delay}ms`,
                  animationDuration: `${d.dur}ms`,
                }}
              />
            ))}
            {minted > 0n && (
              <span className="fx-coin">
                <CoinIcon />+{fmt(minted, 18, 1)}
              </span>
            )}
          </span>
        )}
        {fx?.kind === 'dry' && (
          <span className="plot-fx fx-dry" key={fx.seq} aria-hidden>
            <span className="fx-burn">−{evapPct}%</span>
          </span>
        )}
      </span>

      <span className="plot-sign">{name}</span>
      <span className="plot-stats">
        staked {fmt(city.value)}
        {jackpot > 0n && (
          <span className="plot-jackpot">
            <CoinIcon /> {fmt(jackpot)}
          </span>
        )}
      </span>
    </button>
  );
}
