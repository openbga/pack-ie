/**
 * Work done in steps. The mock computes on the page's own thread, where the engine would use
 * another process, so a search of the whole history is cut into steps and the page's other work
 * (input, painting) runs between them. A step is a stretch of a generator up to its `yield`.
 */

/** A computation done in steps: each `yield` is a place where it may pause. */
export type Steps<T> = Generator<void, T, void>;

/**
 * Items looked at before a pause: some 10 to 20 ms of the slowest filter (delivery states), and
 * far less of the others.
 */
export const STEP = 10_000;

/** Runs `steps` to its end without pausing. */
export function finish<T>(steps: Steps<T>): T {
  for (;;) {
    const next = steps.next();
    if (next.done === true) return next.value;
  }
}

/** Lets the page run its own tasks before the next step. */
export function letPageRun(): Promise<void> {
  const scheduler = (globalThis as { scheduler?: { yield?: () => Promise<void> } }).scheduler;
  return scheduler?.yield?.() ?? new Promise((resolve) => setTimeout(resolve, 0));
}

/** Calls `each(k)` for k from 0 to `count`, pausing after every `STEP` of them. */
export function* eachInSteps(count: number, each: (k: number) => void): Steps<void> {
  for (let from = 0; from < count; from += STEP) {
    const stop = Math.min(count, from + STEP);
    for (let k = from; k < stop; k++) each(k);
    if (stop < count) yield;
  }
}

/** Keys sorted at once by the native sort, and the outputs of a merge made before a pause. */
const SORT_RUN = 50_000;
const MERGE_STEP = 200_000;

/** Merges the sorted `source[left, middle)` and `source[middle, right)` into `target`. */
function* merge(
  source: Float64Array,
  target: Float64Array,
  left: number,
  middle: number,
  right: number,
): Steps<void> {
  let a = left;
  let b = middle;
  let out = left;
  while (a < middle && b < right) {
    const stop = Math.min(out + MERGE_STEP, right);
    while (out < stop && a < middle && b < right) {
      const low = source[a] as number;
      const high = source[b] as number;
      target[out++] = low <= high ? low : high;
      if (low <= high) a++;
      else b++;
    }
    yield;
  }
  target.set(source.subarray(a, middle), out);
  target.set(source.subarray(b, right), out + (middle - a));
}

/**
 * Sorts `values` ascending in steps and returns the sorted array: runs of `SORT_RUN` are sorted
 * natively, then merged in pairs until one is left. All values differ, so the order is the one
 * the native sort of the whole array gives, which is one task of some 100 ms that cannot pause.
 */
export function* sortInSteps(values: Float64Array<ArrayBuffer>): Steps<Float64Array<ArrayBuffer>> {
  const n = values.length;
  if (n <= SORT_RUN) return values.sort();
  for (let from = 0; from < n; from += SORT_RUN) {
    values.subarray(from, Math.min(n, from + SORT_RUN)).sort();
    if (from + SORT_RUN < n) yield;
  }
  let source = values;
  let target = new Float64Array(n);
  for (let width = SORT_RUN; width < n; width *= 2) {
    for (let left = 0; left < n; left += 2 * width) {
      const middle = Math.min(n, left + width);
      yield* merge(source, target, left, middle, Math.min(n, left + 2 * width));
    }
    [source, target] = [target, source];
  }
  return source;
}
