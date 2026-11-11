'use client';

import { useMemo } from 'react';
import { geoEqualEarth, geoGraticule10, geoPath } from 'd3-geo';
import { feature } from 'topojson-client';
import world from 'world-atlas/countries-110m.json';
import type { FeatureCollection, Geometry, GeoJsonProperties, Polygon } from 'geojson';
import type { GeometryCollection, Topology } from 'topojson-specification';
import { useDrizzle } from '../lib/useDrizzle';
import { cityName } from '../lib/cities';
import { approx, fmtMm, fmtAmount, toBig } from '../lib/types';

const WIDTH = 960;
const HEIGHT = 470;

function pinRadius(value: string, pending: string): number {
  const staked = approx(toBig(value) + toBig(pending));
  return Math.min(20, Math.max(4, 4 + Math.sqrt(Math.max(0, staked)) / 7));
}

export function WorldMap() {
  const { cities, round, selected, setSelected, rainNow } = useDrizzle();

  const { path, projection, lands } = useMemo(() => {
    // Fit the inhabited latitudes: Antarctica would waste a third of the canvas.
    const frame: Polygon = {
      type: 'Polygon',
      coordinates: [
        [
          [-180, -58],
          [180, -58],
          [180, 84],
          [-180, 84],
          [-180, -58],
        ],
      ],
    };
    const proj = geoEqualEarth().fitExtent(
      [
        [14, 14],
        [WIDTH - 14, HEIGHT - 14],
      ],
      frame,
    );
    const topo = world as unknown as Topology<{ countries: GeometryCollection }>;
    const collection = feature(topo, topo.objects.countries) as unknown as FeatureCollection<Geometry, GeoJsonProperties>;
    return { projection: proj, path: geoPath(proj), lands: collection };
  }, []);

  const graticule = useMemo(() => path(geoGraticule10()) ?? '', [path]);
  const rainingNow = cities.filter((city) => (rainNow?.[city.index] ?? 0) > 0).length;

  return (
    <div className="map-wrap">
      <svg
        className="map-svg"
        viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
        role="img"
        aria-label="World map with one pin per Drizzle city; pin size is staked DRIZZLE"
      >
        <defs>
          <radialGradient id="ocean" cx="50%" cy="45%" r="75%">
            <stop offset="0%" stopColor="#0d2144" />
            <stop offset="60%" stopColor="#081428" />
            <stop offset="100%" stopColor="#050b16" />
          </radialGradient>
          <filter id="pinGlow" x="-80%" y="-80%" width="260%" height="260%">
            <feGaussianBlur stdDeviation="3.2" result="blur" />
            <feMerge>
              <feMergeNode in="blur" />
              <feMergeNode in="SourceGraphic" />
            </feMerge>
          </filter>
        </defs>

        <rect x="0" y="0" width={WIDTH} height={HEIGHT} fill="url(#ocean)" rx="14" />
        <path d={graticule} className="map-graticule" />

        <g className="map-land">
          {lands.features.map((shape, i) => (
            <path key={i} d={path(shape) ?? ''} />
          ))}
        </g>

        {cities.map((city) => {
          const point = projection([city.lon_e4 / 1e4, city.lat_e4 / 1e4]);
          if (!point) return null;
          const [x, y] = point;
          const radius = pinRadius(city.value, city.pending_total);
          const rainedLastRound = city.last_rain_round !== null && round !== null && city.last_rain_round === round.next_round - 1;
          const wet = (rainNow?.[city.index] ?? 0) > 0;
          const isSelected = selected === city.index;
          const name = cityName(city.index, city.name);
          const label = `${name}: ${fmtAmount(city.value)} DRIZZLE staked, ${city.planters} planters, ${
            city.last_rain_round === null ? 'never rained' : `last rain round ${city.last_rain_round} (${fmtMm(city.last_mm_tenths)} mm)`
          }`;
          return (
            <g
              key={city.index}
              transform={`translate(${x.toFixed(2)},${y.toFixed(2)})`}
              className={`pin ${isSelected ? 'is-selected' : ''} ${wet ? 'is-wet' : ''}`}
              onClick={() => setSelected(city.index)}
              onKeyDown={(event) => {
                if (event.key === 'Enter' || event.key === ' ') {
                  event.preventDefault();
                  setSelected(city.index);
                }
              }}
              role="button"
              tabIndex={0}
              aria-label={label}
            >
              <circle className="pin-hit" r={radius + 9} />
              {rainedLastRound && <circle className="pin-pulse" r={radius + 6} />}
              <circle className="pin-halo" r={radius + 3} />
              <circle className="pin-dot" r={radius} filter="url(#pinGlow)" />
              <circle className="pin-core" r={Math.max(1.4, radius * 0.3)} />
              <text className="pin-label" y={-radius - 7}>
                {name}
              </text>
              {wet && (
                <text className="pin-drop" y={radius + 13} aria-hidden>
                  💧
                </text>
              )}
              <title>{label}</title>
            </g>
          );
        })}
      </svg>

      <p className="map-legend">
        <span>
          <i className="legend-dot" /> pin size = staked $DRIZZLE
        </span>
        <span>
          <i className="legend-pulse" /> rained last round
        </span>
        <span>
          💧 Open-Meteo now{rainingNow > 0 ? ` · ${rainingNow} wet` : ''}
        </span>
      </p>
    </div>
  );
}

export function CityChips() {
  const { cities, selected, setSelected, rainNow } = useDrizzle();
  return (
    <ul className="city-chips" aria-label="Cities">
      {cities.map((city) => {
        const wet = (rainNow?.[city.index] ?? 0) > 0;
        return (
          <li key={city.index}>
            <button
              className={`city-chip ${selected === city.index ? 'is-selected' : ''}`}
              onClick={() => setSelected(city.index)}
              aria-pressed={selected === city.index}
            >
              <span className="chip-name">
                {cityName(city.index, city.name)}
                {wet && <span className="chip-wet" aria-label="raining now">💧</span>}
              </span>
              <span className="chip-stat">{fmtAmount(city.value)} staked</span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}
