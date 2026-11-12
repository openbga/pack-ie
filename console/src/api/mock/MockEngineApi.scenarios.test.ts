import { describe, expect, it } from "vitest";
import { mockApiFactory } from "./mockTestSupport";

const create = mockApiFactory({ seed: 3 });

describe("the mock's demo scenarios", () => {
  it("a fresh install has no endpoints, channels, messages or alerts", async () => {
    const api = create({ empty: true });
    const snapshot = await api.getSnapshot();
    expect(snapshot.channels).toEqual([]);
    expect(snapshot.endpoints).toEqual([]);
    expect(snapshot.openAlerts).toEqual([]);
    const page = await api.listMessages({
      filter: {},
      sort: { field: "receivedAt", descending: true },
      offset: 0,
      limit: 50,
    });
    expect(page.total).toBe(0);
    expect(await api.listAlerts("all")).toEqual([]);
    const config = await api.getConfig();
    expect(config.channels).toEqual([]);
    expect((await api.queryLogs({}, { before: null, limit: 10 })).events).toBeDefined();
  });

  it("can fail every request, except asking whether an engine is there", async () => {
    const api = create({ failReads: true });
    await expect(api.getSnapshot()).rejects.toMatchObject({ code: "internal" });
    await expect(api.getConfig()).rejects.toMatchObject({ code: "internal" });
    expect(await api.getConnection()).toMatchObject({ state: "connected" });
  });
});
