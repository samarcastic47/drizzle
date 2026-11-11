/**
 * Open-Meteo calls made straight from the browser.
 * The verify call must hit the SAME URL shape the oracle uses, so the lat/lon
 * formatter is literally (x / 1e4).toString().
 */

import type { CityConfig } from './types';

const FORECAST = 'https://api.open-meteo.com/v1/forecast';

/** (x/1e4).toString() — no rounding tricks, matches the oracle's formatting. */
export function coord(e4: number): string {
  return (e4 / 1e4).toString();
}

export function latCsv(cities: CityConfig[]): string {
  return cities.map((c) => coord(c.lat_e4)).join(',');
}

export function lonCsv(cities: CityConfig[]): string {
  return cities.map((c) => coord(c.lon_e4)).join(',');
}

export function currentWeatherUrl(cities: CityConfig[]): string {
  return `${FORECAST}?latitude=${latCsv(cities)}&longitude=${lonCsv(cities)}&current=precipitation&timezone=UTC`;
}

export function hourlyVerifyUrl(cities: CityConfig[], hourIso: string): string {
  return (
    `${FORECAST}?latitude=${latCsv(cities)}&longitude=${lonCsv(cities)}` +
    `&hourly=precipitation&start_hour=${hourIso}&end_hour=${hourIso}&timezone=UTC`
  );
}

type AnyRecord = Record<string, unknown>;

function pickArray(payload: unknown): AnyRecord[] {
  if (Array.isArray(payload)) return payload as AnyRecord[];
  if (payload && typeof payload === 'object') return [payload as AnyRecord];
  return [];
}

function num(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null;
}

/** mm per city for the current moment; array parallel to cities. */
export async function fetchCurrentRain(cities: CityConfig[], signal?: AbortSignal): Promise<(number | null)[]> {
  const res = await fetch(currentWeatherUrl(cities), { signal, cache: 'no-store' });
  if (!res.ok) throw new Error(`Open-Meteo ${res.status}`);
  const data = pickArray(await res.json());
  if (data.length !== cities.length) throw new Error(`Open-Meteo returned ${data.length} locations`);
  return data.map((d) => {
    const current = (d.current ?? {}) as AnyRecord;
    return num(current.precipitation);
  });
}

export interface HourlyReading {
  name: string;
  mm: number | null;
  tenths: number;
}

/**
 * Re-fetch one settled hour for every city — the same query the oracle makes.
 * Returns tenths = Math.round(mm * 10), null -> 0 (as the oracle does).
 */
export async function fetchHourlyReadings(
  cities: CityConfig[],
  hourIso: string,
  signal?: AbortSignal,
): Promise<{ url: string; readings: HourlyReading[] }> {
  const url = hourlyVerifyUrl(cities, hourIso);
  const res = await fetch(url, { signal, cache: 'no-store' });
  if (!res.ok) throw new Error(`Open-Meteo ${res.status}`);
  const data = pickArray(await res.json());
  if (data.length !== cities.length) throw new Error(`Open-Meteo returned ${data.length} locations`);
  const readings = data.map((d, i) => {
    const hourly = (d.hourly ?? {}) as AnyRecord;
    const series = Array.isArray(hourly.precipitation) ? hourly.precipitation : [];
    const mm = num(series[0]);
    return {
      name: cities[i]?.name ?? `city ${i}`,
      mm,
      tenths: Math.round((mm ?? 0) * 10),
    };
  });
  return { url, readings };
}
