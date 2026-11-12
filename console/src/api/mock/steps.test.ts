import { describe, expect, it } from "vitest";
import { MockEngineApi } from "./index";
import { eachInSteps, finish, STEP, sortInSteps } from "./steps";

/** Different numbers in no particular order, as the sort keys are (each carries its index). */
function keys(count: number): Float64Array<ArrayBuffer> {
  const values = new Float64Array(count);
  let state = 12345;
  for (let i = 0; i < count; i++) {
    state = (state * 1103515245 + 12345) % 2147483648;
    values[i] = Math.floor(state / 64) * 4_194_304 + i;
  }
  return values;
}

describe("sortInSteps", () => {
  it.each([0, 1, 49_999, 50_000, 50_001, 130_000, 300_001])(
    "gives the order of the native sort for %i values",
    (count) => {
      const values = keys(count);
      const expected = Float64Array.from(values).sort();
      expect(finish(sortInSteps(values))).toEqual(expected);
    },
  );

  it("pauses between its steps and not before them", () => {
    const steps = sortInSteps(keys(130_000));
    let pauses = 0;
    for (let next = steps.next(); next.done !== true; next = steps.next()) pauses++;
    expect(pauses).toBeGreaterThan(2);
    const small = sortInSteps(keys(1000));
    expect(small.next().done).toBe(true);
  });
});

describe("eachInSteps", () => {
  it("visits every item once, in order, pausing every step", () => {
    const seen: number[] = [];
    const steps = eachInSteps(STEP * 2 + 5, (k) => seen.push(k));
    let pauses = 0;
    for (let next = steps.next(); next.done !== true; next = steps.next()) pauses++;
    expect(pauses).toBe(2);
    expect(seen).toHaveLength(STEP * 2 + 5);
    expect(seen.every((k, i) => k === i)).toBe(true);
  });
});

describe("a query over a history of many steps", () => {
  it("is answered in order, whichever way it is sorted", async () => {
    const api = new MockEngineApi({ latency: "none", messageCount: 120_000, seed: 3 });
    try {
      const query = (field: "sizeBytes" | "state" | "receivedAt", descending: boolean) => ({
        filter: { deliverySummaries: ["failed", "attention"] as never },
        sort: { field, descending },
        offset: 0,
        limit: 1000,
      });
      const ascending = await api.listMessages(query("sizeBytes", false));
      const descendingPage = await api.listMessages(query("sizeBytes", true));
      expect(ascending.total).toBeGreaterThan(1000);
      expect(descendingPage.total).toBe(ascending.total);
      const sizes = ascending.rows.map((row) => row.sizeBytes);
      expect(sizes).toEqual([...sizes].sort((a, b) => a - b));
      const largest = descendingPage.rows.map((row) => row.sizeBytes);
      expect(largest).toEqual([...largest].sort((a, b) => b - a));
      expect(largest[0]).toBeGreaterThanOrEqual(sizes.at(-1) ?? 0);
      // Equal sizes, newest first: the same order the whole sort gives.
      const newest = await api.listMessages(query("receivedAt", true));
      const times = newest.rows.map((row) => Date.parse(row.receivedAt));
      expect(times).toEqual([...times].sort((a, b) => b - a));
    } finally {
      api.dispose();
    }
  }, 60_000);
});
