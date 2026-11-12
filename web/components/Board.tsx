'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useDrizzle } from '../lib/useDrizzle';
import { cityName } from '../lib/cities';
import { onPlantFlight, prefersReducedMotion } from '../lib/fx';
import { fmtMm, toBig, type UserCity } from '../lib/types';
import { Plot, type PlotFx } from './Plot';

const SETTLE_STAGGER = 60;
const FX_LIFE = 2400;

const SEED_SVG =
  '<svg viewBox="0 0 20 20" width="18" height="18" aria-hidden="true">' +
  '<path d="M10 18c-3.9 0-7-3.1-7-7C3 6 10 2.5 10 2.5S17 6 17 11c0 3.9-3.1 7-7 7z" fill="#e6d3a8"/>' +
  '<path d="M10 8v7" stroke="#1b0f05" stroke-width="1.3" stroke-linecap="round" opacity=".5"/>' +
  '</svg>';

export function Board() {
  const { cities, round, logs, config, selected, setSelected, user } = useDrizzle();
  const [changed, setChanged] = useState<Set<number>>(new Set());
  const [fxMap, setFxMap] = useState<Record<number, PlotFx>>({});
  const [announce, setAnnounce] = useState('');

  const lastRound = useRef<number | null>(null);
  const seq = useRef(0);
  const timers = useRef<number[]>([]);

  const markSeen = useCallback((index: number) => {
    setChanged((prev) => {
      if (!prev.has(index)) return prev;
      const nextSet = new Set(prev);
      nextSet.delete(index);
      return nextSet;
    });
  }, []);

  useEffect(
    () => () => {
      timers.current.forEach((t) => window.clearTimeout(t));
      timers.current = [];
    },
    [],
  );

  /* ---------- settlement moment ---------- */
  useEffect(() => {
    if (!logs.length || !round) return;
    const newest = logs.reduce((a, b) => (a.round > b.round ? a : b));
    if (lastRound.current === null) {
      lastRound.current = newest.round;
      return;
    }
    if (newest.round <= lastRound.current) return;
    lastRound.current = newest.round;

    const threshold = config?.rain_threshold_tenths ?? 2;
    const reduce = prefersReducedMotion();
    const touched: number[] = [];
    let totalMinted = 0n;

    newest.mm_tenths.forEach((mm, index) => {
      const minted = newest.minted[index] ?? '0';
      const burned = newest.burned[index] ?? '0';
      const isWet = mm >= threshold && mm > 0;
      const isDry = toBig(burned) > 0n;
      if (!isWet && !isDry) return;
      touched.push(index);
      totalMinted += toBig(minted);
      if (reduce) return;

      const id = ++seq.current;
      const at = index * SETTLE_STAGGER;
      timers.current.push(
        window.setTimeout(() => {
          setFxMap((prev) => ({ ...prev, [index]: { kind: isWet ? 'rain' : 'dry', minted, seq: id } }));
        }, at),
        window.setTimeout(() => {
          setFxMap((prev) => {
            if (prev[index]?.seq !== id) return prev;
            const nextMap = { ...prev };
            delete nextMap[index];
            return nextMap;
          });
        }, at + FX_LIFE),
      );
    });

    // The glow ring appears once the moment has played, so it reads as "look back at these".
    const markAt = reduce ? 0 : touched.length * SETTLE_STAGGER + 900;
    timers.current.push(
      window.setTimeout(() => {
        if (touched.length > 0) setChanged(new Set(touched));
      }, markAt),
    );

    const wetNames = newest.mm_tenths
      .map((mm, i) => (mm >= threshold && mm > 0 ? `${cityName(i)} ${fmtMm(mm)} mm` : null))
      .filter(Boolean);
    setAnnounce(
      `Round ${newest.round} settled. ${wetNames.length ? wetNames.join(', ') : 'No city passed the rain threshold'}. ${
        totalMinted > 0n ? 'New coins minted to planters.' : ''
      }`,
    );
  }, [logs, round, config]);

  /* ---------- plant flight ---------- */
  useEffect(
    () =>
      onPlantFlight((cityIndex, from) => {
        const plot = document.querySelector<HTMLElement>(`[data-plot="${cityIndex}"]`);
        const soil = plot?.querySelector<HTMLElement>('.plot-soil');
        if (!plot || !soil) return;
        if (prefersReducedMotion()) {
          plot.classList.add('is-sprouting');
          window.setTimeout(() => plot.classList.remove('is-sprouting'), 400);
          return;
        }

        const to = soil.getBoundingClientRect();
        const startX = from.left + from.width / 2;
        const startY = from.top + from.height / 2;
        const endX = to.left + to.width / 2;
        const endY = to.top + to.height * 0.52;

        const seed = document.createElement('div');
        seed.className = 'seed-flight';
        seed.style.left = `${startX}px`;
        seed.style.top = `${startY}px`;
        seed.innerHTML = SEED_SVG;
        document.body.appendChild(seed);

        const midX = (startX + endX) / 2 + (endX > startX ? 46 : -46);
        const midY = Math.max(72, Math.min(startY, endY) - 120);
        const flight = seed.animate(
          [
            { transform: 'translate(-50%, -50%) scale(.55) rotate(0deg)', offset: 0 },
            {
              transform: `translate(calc(-50% + ${midX - startX}px), calc(-50% + ${midY - startY}px)) scale(1) rotate(150deg)`,
              offset: 0.55,
            },
            {
              transform: `translate(calc(-50% + ${endX - startX}px), calc(-50% + ${endY - startY}px)) scale(.8) rotate(260deg)`,
              offset: 1,
            },
          ],
          { duration: 700, easing: 'cubic-bezier(.22,.9,.3,1)', fill: 'forwards' },
        );

        flight.onfinish = () => {
          seed.remove();
          puff(endX, endY);
          plot.classList.add('is-sprouting');
          window.setTimeout(() => plot.classList.remove('is-sprouting'), 760);
        };
        flight.oncancel = () => seed.remove();
      }),
    [],
  );

  const evapPct = ((config?.evap_bps ?? 10) / 100).toFixed(2);
  const mineByCity = useMemo(() => {
    const map = new Map<number, UserCity>();
    for (const entry of user?.cities ?? []) map.set(entry.index, entry);
    return map;
  }, [user]);

  return (
    <section className="board-wrap" aria-label="City plots">
      <h2 className="sr-only">12 city plots</h2>
      <div className="board">
        {cities.map((city) => (
          <Plot
            key={city.index}
            city={city}
            mine={mineByCity.get(city.index) ?? null}
            selected={selected === city.index}
            changed={changed.has(city.index)}
            fx={fxMap[city.index]}
            evapPct={evapPct}
            roundNumber={round?.next_round ?? null}
            onSelect={setSelected}
            onSeen={markSeen}
          />
        ))}
      </div>
      <p className="sr-only" role="status" aria-live="polite">
        {announce}
      </p>
    </section>
  );
}

/** Three dust rings expanding from the landing point. */
function puff(x: number, y: number): void {
  for (let i = 0; i < 4; i += 1) {
    const dust = document.createElement('div');
    dust.className = 'soil-puff';
    dust.style.left = `${x + (i - 1.5) * 11}px`;
    dust.style.top = `${y + (i % 2 === 0 ? 2 : -3)}px`;
    document.body.appendChild(dust);
    const anim = dust.animate(
      [
        { transform: 'translate(-50%, -50%) scale(.3)', opacity: 0.9, offset: 0 },
        { transform: `translate(calc(-50% + ${(i - 1.5) * 16}px), -50%) scale(1.6)`, opacity: 0.35, offset: 0.6 },
        { transform: `translate(calc(-50% + ${(i - 1.5) * 22}px), -60%) scale(2.1)`, opacity: 0, offset: 1 },
      ],
      { duration: 400 + i * 40, easing: 'cubic-bezier(.16,1,.3,1)', fill: 'forwards' },
    );
    anim.onfinish = () => dust.remove();
  }
}
