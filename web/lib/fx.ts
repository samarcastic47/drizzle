/**
 * Tiny one-shot animation bus: the action card says "a seed left the button",
 * the board (which owns the plot coordinates) plays the flight.
 */
export type PlantFlight = (city: number, from: DOMRect) => void;

let handler: PlantFlight | null = null;

export function onPlantFlight(next: PlantFlight): () => void {
  handler = next;
  return () => {
    if (handler === next) handler = null;
  };
}

export function flyPlant(city: number, el: HTMLElement | null): void {
  if (!handler || !el) return;
  handler(city, el.getBoundingClientRect());
}

export function prefersReducedMotion(): boolean {
  return (
    typeof window !== 'undefined' &&
    window.matchMedia('(prefers-reduced-motion: reduce)').matches
  );
}
