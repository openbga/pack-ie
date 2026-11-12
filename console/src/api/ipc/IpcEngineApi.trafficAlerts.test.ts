import { describe, expect, it } from "vitest";
import { IDS } from "./test/configFixtures";
import { settle } from "./test/fakeBridge";
import { engine } from "./test/ipcEngine";
import { engineAlert } from "./test/snapshotFixtures";

describe("live traffic", () => {
  it("delivers segments with the direction the channel's document gives them", async () => {
    const { bridge, api } = engine();
    const received: unknown[] = [];
    const stop = api.subscribeTraffic(IDS.bridge, (segments) => received.push(...segments));
    await settle();
    expect(bridge.calls.find((c) => c.command === "ipc_subscribe")?.args.topic).toEqual({
      type: "traffic",
      channel_id: IDS.bridge,
    });
    bridge.emit("packie://event", {
      subscription: 1,
      event: {
        type: "traffic",
        segment: {
          channel_id: IDS.bridge,
          endpoint_id: IDS.client,
          direction: "inbound",
          role: "received",
          destination_id: null,
          at: "2026-11-06T12:00:00Z",
          bytes: [0x41],
        },
      },
    });
    await settle(10);
    expect(received).toEqual([
      {
        seq: 1,
        at: "2026-11-06T12:00:00Z",
        direction: "b_to_a",
        base64: "QQ==",
        flow: "inbound",
        endpointId: IDS.client,
        role: "received",
        destinationId: null,
      },
    ]);
    stop();
  });

  it("reports a refused transcript through onError instead of throwing", async () => {
    const { bridge, api } = engine();
    bridge.command("ipc_subscribe", () => {
      throw {
        kind: "ipc",
        error: {
          code: "rejected",
          message: "Transcript in use by another client.",
          field_errors: [],
          reason_code: "transcript_in_use",
        },
      };
    });
    const errors: unknown[] = [];
    expect(() =>
      api.subscribeTraffic(
        IDS.bridge,
        () => undefined,
        (error) => errors.push(error),
      ),
    ).not.toThrow();
    await settle(10);
    expect(errors).toEqual([
      {
        code: "conflict",
        message: "Transcript in use by another client.",
        fieldErrors: [],
        rejection: "transcript_in_use",
        correlationId: "",
      },
    ]);
  });
});

describe("alerts", () => {
  const opened = (id: string) => engineAlert(id, "warning", { summary: `alert ${id}` });
  const cleared = (id: string, at: string) =>
    engineAlert(id, "warning", { state: "cleared", cleared_at: at });

  function history() {
    const ctx = engine();
    ctx.bridge.request("get_alerts", (request) => {
      if (request.type !== "get_alerts") throw new Error("unexpected");
      // The engine answers active alerts first, then cleared ones, each newest first.
      const alerts = [
        engineAlert("minor", "warning"),
        engineAlert("major", "critical"),
        ...(request.include_cleared
          ? [cleared("new", "2026-11-06T11:40:00Z"), cleared("old", "2026-11-06T11:20:00Z")]
          : []),
      ];
      return { type: "alerts", alerts };
    });
    return ctx;
  }

  it("asks for open alerts only and lists them most severe first", async () => {
    const { bridge, api } = history();
    expect((await api.listAlerts("open")).map((a) => a.id)).toEqual(["major", "minor"]);
    expect(bridge.sent("get_alerts")).toEqual([
      { type: "get_alerts", include_cleared: false, since: null },
    ]);
    expect(bridge.count("ipc_subscribe")).toBe(0);
  });

  it("asks the engine for the history instead of collecting it", async () => {
    const { bridge, api } = history();
    const all = await api.listAlerts("all");
    expect(all.map((a) => [a.id, a.state])).toEqual([
      ["major", "open"],
      ["minor", "open"],
      ["new", "cleared"],
      ["old", "cleared"],
    ]);
    expect(all[2]?.clearedAt).toBe("2026-11-06T11:40:00Z");
    expect((await api.listAlerts("cleared")).map((a) => a.id)).toEqual(["new", "old"]);
    expect(bridge.sent("get_alerts")).toEqual([
      { type: "get_alerts", include_cleared: true, since: null },
      { type: "get_alerts", include_cleared: true, since: null },
    ]);
    expect(bridge.count("ipc_subscribe")).toBe(0);
  });

  it("maps an alert's fields", async () => {
    const { bridge, api } = engine();
    bridge.request("get_alerts", () => ({ type: "alerts", alerts: [opened("a")] }));
    expect(await api.listAlerts("open")).toEqual([
      {
        id: "a",
        severity: "warning",
        kind: "endpoint_down",
        title: "alert a",
        detail: "count: 2, peer: 10.0.0.9",
        channelId: null,
        endpointId: IDS.client,
        firstSeenAt: "2026-11-06T11:00:00.000Z",
        lastSeenAt: "2026-11-06T11:30:00.000Z",
        occurrences: 3,
        state: "open",
        clearedAt: null,
        nextAction: "Check the cable",
      },
    ]);
  });

  it("rejects with the engine's refusal", async () => {
    const { bridge, api } = engine();
    bridge.request("get_alerts", () => {
      throw { kind: "disconnected", message: "gone" };
    });
    await expect(api.listAlerts("all")).rejects.toMatchObject({ code: "engine_unavailable" });
  });
});
