import { describe, expect, it } from "vitest";
import type { TapSegment } from "@/ipc/generated";
import { IpcClient } from "./client";
import realConfig from "./real/config.json";
import { IDS, richConfig } from "./test/configFixtures";
import { FakeBridge, settle } from "./test/fakeBridge";
import { refusableSubscribe } from "./test/pendingSubscribe";
import { subscribeTraffic, trafficDirection } from "./traffic";
import type { WireChannel } from "./wire";

const bridgeConfig = richConfig().channels?.[0] as WireChannel;
const messageConfig = richConfig().channels?.[1] as WireChannel;

/** The tap derives the role from the direction on bridges (received/sent); the pane ignores it there. */
function segment(
  endpoint: string,
  direction: TapSegment["direction"],
  bytes = [0x41],
  role: TapSegment["role"] = direction === "inbound" ? "received" : "sent",
  destination: string | null = null,
): TapSegment {
  return {
    channel_id: IDS.bridge,
    endpoint_id: endpoint,
    direction,
    role,
    destination_id: destination,
    at: "2026-11-06T12:00:00Z",
    bytes,
  };
}

describe("trafficDirection", () => {
  it.each([
    ["from A", IDS.serial, "inbound", "a_to_b"],
    ["to B", IDS.client, "outbound", "a_to_b"],
    ["from B", IDS.client, "inbound", "b_to_a"],
    ["to A", IDS.serial, "outbound", "b_to_a"],
  ] as const)("bridge bytes %s", (_name, endpoint, direction, expected) => {
    expect(trafficDirection(segment(endpoint, direction), bridgeConfig)).toBe(expected);
  });

  it.each([
    ["received from the source", IDS.listener, "inbound", "received", null, "a_to_b"],
    ["the engine's response to the source", IDS.listener, "outbound", "response", null, "a_to_b"],
    ["sent to a destination", IDS.client, "outbound", "sent", IDS.hl7, "b_to_a"],
    ["a destination's reply", IDS.client, "inbound", "reply", IDS.hl7, "b_to_a"],
    ["sent to another destination", IDS.folderOut, "outbound", "sent", IDS.folder, "b_to_a"],
  ] as const)(
    "message channel segment %s",
    (_name, endpoint, direction, role, destination, pane) => {
      expect(
        trafficDirection(segment(endpoint, direction, [0x41], role, destination), messageConfig),
      ).toBe(pane);
    },
  );

  it("reads inbound as A to B when the channel's document is not known", () => {
    expect(trafficDirection(segment(IDS.listener, "inbound"), undefined)).toBe("a_to_b");
    expect(trafficDirection(segment(IDS.listener, "outbound"), undefined)).toBe("b_to_a");
  });
});

describe("subscribeTraffic", () => {
  it("subscribes to the channel and delivers numbered segments with base64 bytes", async () => {
    const bridge = new FakeBridge()
      .command("ipc_subscribe", () => 9)
      .command("ipc_unsubscribe", () => null);
    const received: unknown[][] = [];
    const stop = subscribeTraffic(
      new IpcClient(bridge),
      IDS.bridge,
      () => Promise.resolve(bridgeConfig),
      (segments) => received.push(segments),
    );
    await settle();
    expect(bridge.calls.find((c) => c.command === "ipc_subscribe")?.args.topic).toEqual({
      type: "traffic",
      channel_id: IDS.bridge,
    });
    bridge.emit("packie://event", {
      subscription: 9,
      event: { type: "traffic", segment: segment(IDS.serial, "inbound", [0x05, 0x02, 0x41, 0xff]) },
    });
    bridge.emit("packie://event", {
      subscription: 9,
      event: { type: "traffic", segment: segment(IDS.serial, "outbound", [0x06]) },
    });
    await settle(10);
    expect(received.flat()).toEqual([
      {
        seq: 1,
        at: "2026-11-06T12:00:00Z",
        direction: "a_to_b",
        base64: "BQJB/w==",
        flow: "inbound",
        endpointId: IDS.serial,
        role: "received",
        destinationId: null,
      },
      {
        seq: 2,
        at: "2026-11-06T12:00:00Z",
        direction: "b_to_a",
        base64: "Bg==",
        flow: "outbound",
        endpointId: IDS.serial,
        role: "sent",
        destinationId: null,
      },
    ]);
    stop();
    bridge.emit("packie://event", {
      subscription: 9,
      event: { type: "traffic", segment: segment(IDS.serial, "inbound") },
    });
    await settle(10);
    expect(received.flat()).toHaveLength(2);
    expect(bridge.count("ipc_unsubscribe")).toBe(1);
  });

  it("sorts a message channel's segments into the Source and Destinations panes", async () => {
    const bridge = new FakeBridge().command("ipc_subscribe", () => 3);
    const received: unknown[][] = [];
    subscribeTraffic(
      new IpcClient(bridge),
      IDS.message,
      () => Promise.resolve(messageConfig),
      (segments) => received.push(segments),
    );
    await settle();
    for (const [endpoint, direction, role, destination] of [
      [IDS.listener, "inbound", "received", null],
      [IDS.listener, "outbound", "response", null],
      [IDS.client, "outbound", "sent", IDS.hl7],
      [IDS.client, "inbound", "reply", IDS.hl7],
    ] as const) {
      bridge.emit("packie://event", {
        subscription: 3,
        event: {
          type: "traffic",
          segment: segment(endpoint, direction, [0x41], role, destination),
        },
      });
    }
    await settle(10);
    expect(
      received.flat().map((s) => s as { direction: string; flow: string; endpointId: string }),
    ).toMatchObject([
      { direction: "a_to_b", flow: "inbound", role: "received", destinationId: null },
      { direction: "a_to_b", flow: "outbound", role: "response", destinationId: null },
      { direction: "b_to_a", flow: "outbound", role: "sent", destinationId: IDS.hl7 },
      { direction: "b_to_a", flow: "inbound", role: "reply", destinationId: IDS.hl7 },
    ]);
  });

  it("waits for the channel's document before delivering", async () => {
    const bridge = new FakeBridge().command("ipc_subscribe", () => 1);
    let release: (config: WireChannel) => void = () => undefined;
    const received: unknown[][] = [];
    subscribeTraffic(
      new IpcClient(bridge),
      IDS.bridge,
      () =>
        new Promise((resolve) => {
          release = resolve;
        }),
      (segments) => received.push(segments),
    );
    await settle();
    bridge.emit("packie://event", {
      subscription: 1,
      event: { type: "traffic", segment: segment(IDS.client, "inbound") },
    });
    await settle();
    expect(received).toEqual([]);
    release(bridgeConfig);
    await settle(10);
    expect(received.flat()).toMatchObject([{ direction: "b_to_a" }]);
  });

  it("reports a transcript the engine refuses, with the engine's text", async () => {
    const bridge = new FakeBridge().command("ipc_subscribe", () => {
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
    const received: unknown[][] = [];
    subscribeTraffic(
      new IpcClient(bridge),
      IDS.bridge,
      () => Promise.resolve(bridgeConfig),
      (segments) => received.push(segments),
      (error) => errors.push(error),
    );
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
    expect(received).toEqual([]);
  });

  it("does not report a refusal after the viewer left", async () => {
    const { bridge, refuse } = refusableSubscribe();
    const errors: unknown[] = [];
    const stop = subscribeTraffic(
      new IpcClient(bridge),
      IDS.bridge,
      () => Promise.resolve(bridgeConfig),
      () => undefined,
      (error) => errors.push(error),
    );
    await settle(10);
    stop();
    refuse();
    await settle(10);
    expect(errors).toEqual([]);
  });
});

describe("segments a real bridge produced", () => {
  // The tap reports what each endpoint received (`inbound`): `ping` from the instrument on
  // side A, `pong` from the system on side B (docs/spec/diagnostics.md, transcript).
  const realBridge = (realConfig as unknown as { channels: WireChannel[] })
    .channels[0] as WireChannel;
  const text = (bytes: number[]) => String.fromCharCode(...bytes);
  const ping = [112, 105, 110, 103, 32, 49, 52, 48, 10];
  const pong = [112, 111, 110, 103, 32, 49, 52, 48, 10];

  it("reads ping as A to B and pong as B to A", () => {
    const fromA: TapSegment = {
      channel_id: "bridge-1",
      endpoint_id: "inst-side",
      direction: "inbound",
      role: "received",
      destination_id: null,
      at: "2026-11-06T20:22:34.548733000Z",
      bytes: ping,
    };
    const fromB: TapSegment = { ...fromA, endpoint_id: "sys-side", bytes: pong };
    expect(text(ping)).toBe("ping 140\n");
    expect(trafficDirection(fromA, realBridge)).toBe("a_to_b");
    expect(trafficDirection(fromB, realBridge)).toBe("b_to_a");
  });
});
