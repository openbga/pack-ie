/** Deterministic randomness for the mock engine: the same seed always produces the same data. */

function mulberry32(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/**
 * Stateless 32-bit mix of three integers. The lazy message generator derives every field of
 * message `index` from it, so nothing about the 400k messages is ever stored.
 */
export function hash32(seed: number, index: number, salt: number): number {
  let h =
    Math.imul(seed ^ 0x9e3779b9, 0x85ebca6b) ^
    Math.imul(index + 0x7f4a7c15, 0xc2b2ae35) ^
    Math.imul(salt + 1, 0x27d4eb2f);
  h ^= h >>> 16;
  h = Math.imul(h, 0x85ebca6b);
  h ^= h >>> 13;
  h = Math.imul(h, 0xc2b2ae35);
  h ^= h >>> 16;
  return h >>> 0;
}

/** A seeded generator with the conveniences the mock needs. */
export class Rng {
  private readonly next: () => number;

  constructor(seed: number) {
    this.next = mulberry32(seed);
  }

  float(): number {
    return this.next();
  }

  /** Integer in `[min, max]`. */
  int(min: number, max: number): number {
    return min + Math.floor(this.next() * (max - min + 1));
  }

  chance(probability: number): boolean {
    return this.next() < probability;
  }

  pick<T>(items: readonly T[]): T {
    return items[Math.floor(this.next() * items.length)] as T;
  }

  hex(length: number): string {
    let out = "";
    while (out.length < length) out += Math.floor(this.next() * 16).toString(16);
    return out;
  }
}

/** Picks an element by a hash value. */
export function pickBy<T>(items: readonly T[], hash: number): T {
  return items[hash % items.length] as T;
}

/** FNV-1a of a string, to derive per-id seeds. */
export function hashString(text: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}
