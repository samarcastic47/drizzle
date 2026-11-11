/**
 * On-chain city names are ASCII ("Sao Paulo"); the UI shows proper names.
 * Fixed order, keyed by city index (0–11) as defined by the contract.
 */
const DISPLAY_NAMES = [
  'Bergen',
  'São Paulo',
  'Lagos',
  'Singapore',
  'Tokyo',
  'Jakarta',
  'Mumbai',
  'London',
  'Lisbon',
  'Seattle',
  'Cairo',
  'Dubai',
];

export function cityName(index: number, onChainName?: string): string {
  return DISPLAY_NAMES[index] ?? onChainName ?? `City ${index}`;
}
