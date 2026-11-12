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
  return Math.min(18, Math.max(4, 4 + Math.sqrt(Math.max(0, staked)) / 8));
}

export function WorldMap() {
  const { cities, round, selected, setSelected, rainNow } = useDrizzle();

  const { path, projection, lands } = useMemo(() => {
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
        [12, 12],
        [WIDTH - 12, HEIGHT - 12],
      ],
      frame,
    );
    const topo = world as unknown as Topology<{ countries: GeometryCollection }>;
    const collection = feature(topo, topo.objects.countries) as unknown as FeatureCollection<
      Geometry,
      GeoJsonProperties
    >;
    return { projection: proj, path: geoPath(proj), lands: collection };
  }, []);

  const graticule = useMemo(() => path(geoGraticule10()) ?? '', [path]);

  return (
    <div className="map-wrap">
      <svg
        className="map-svg"
        viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
        role="img"
        aria-label="World map with one plot per Drizzle city; pin size follows the staked amount"
      >
        <rect x="0" y="0" width={WIDTH} height={HEIGHT} fill="#e8d3a6" rx="3" />
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
          const rainedLastRound =
            city.last_rain_round !== null && round !== null && city.last_rain_round === round.next_round - 1;
          const wet = (rainNow?.[city.index] ?? 0) > 0;
          const isSelected = selected === city.index;
          const name = cityName(city.index, city.name);
          const label = `${name}: ${fmtAmount(city.value)} DRIZZLE staked, ${city.planters} planters, ${
            city.last_rain_round === null
              ? 'never rained'
              : `last rain round ${city.last_rain_round} (${fmtMm(city.last_mm_tenths)} mm)`
          }`;
          return (
            <g
              key={city.index}
              transform={`translate(${x.toFixed(2)},${y.toFixed(2)})`}
              className={`pin${isSelected ? ' is-selected' : ''}${wet ? ' is-wet' : ''}`}
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
              <circle className="pin-hit" r={radius + 10} />
              {rainedLastRound && <circle className="pin-pulse" r={radius + 6} />}
              <circle className="pin-dot" r={radius} />
              {wet && <circle className="pin-wet" cx={radius + 4} cy={-radius - 2} r="2.6" />}
              <text className="pin-label" y={-radius - 8}>
                {name}
              </text>
              <title>{label}</title>
            </g>
          );
        })}
      </svg>

      <p className="map-legend">
        <span>
          <i className="legend-dot" /> pin size = staked
        </span>
        <span>
          <i className="legend-ring" /> rained last round
        </span>
        <span>
          <i className="legend-wet" /> raining now
        </span>
      </p>
    </div>
  );
}
