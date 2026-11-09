// The arcs around the full stop: sound leaving it. Each page has its own seed,
// so the pages read as a series of posters, and the same page always draws the
// same picture.

export interface Ring {
  /** Radius, in a 200 unit wide drawing centred on the disc. */
  r: number;
  width: number;
  /** How much of the circle is drawn, percent. */
  arc: number;
  /** Where the arc starts, degrees. */
  start: number;
  /** Seconds for a full turn while Murmur listens; the sign is the direction. */
  spin: number;
  /** This one is drawn in amber. */
  hot: boolean;
}

const RADII = [22, 30, 39, 50, 62, 75, 89];
/** Each ring heavier than the last, like sound spreading out from the disc. */
const WIDTHS = [1.2, 1.8, 2.8, 4.2, 6, 8.5, 11.5];

export function ringsFor(seed: number): Ring[] {
  // A small linear congruential generator: enough to vary a picture.
  let state = (Math.imul(seed + 1, 2654435761) >>> 0) || 1;
  const next = () => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    return state / 2 ** 32;
  };
  const hot = 2 + Math.floor(next() * (RADII.length - 2));
  return RADII.map((r, i) => {
    const arc = 26 + next() * 48;
    const start = Math.floor(next() * 360);
    const seconds = 7 + next() * 15;
    return { r, width: WIDTHS[i], arc, start, spin: i % 2 === 0 ? seconds : -seconds, hot: i === hot };
  });
}
