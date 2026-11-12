import { describe, expect, it } from "vitest";
import type { BridgeChannelConfig } from "../types";
import { bridgeFromWire, bridgeToWire } from "./bridgeConfig";
import { issueList } from "./connectorConfig";
import { documentProblems } from "./test/specSchema";
import type { WireBridge } from "./wire";

const RAW: WireBridge = {
  channel_id: "br-1",
  label: "Bridge",
  kind: "bridge",
  endpoint_a: "a",
  endpoint_b: "b",
};

const write = (ui: BridgeChannelConfig, raw: WireBridge | undefined = RAW) =>
  bridgeToWire(ui, raw, "channels[0]", issueList());

describe("a bridge tap's retention", () => {
  it("reads the tap's own defaults: 30 days, no count limit", () => {
    expect(bridgeFromWire(RAW).tapRetention).toEqual({ days: 30, maxMessages: null });
    expect(bridgeFromWire({ ...RAW, tap: { framer: "astm" } }).tapRetention).toEqual({
      days: 30,
      maxMessages: null,
    });
  });

  it("reads both limits, and null as no age limit", () => {
    const raw: WireBridge = {
      ...RAW,
      tap: { retention: { max_age_days: null, max_messages: 5000 } },
    };
    expect(bridgeFromWire(raw).tapRetention).toEqual({ days: null, maxMessages: 5000 });
  });

  it("writes only the limit that changed and keeps the tap's other keys", () => {
    const raw: WireBridge = {
      ...RAW,
      tap: { framer: "hl7_mllp", record_messages: true, max_message_bytes: 4096 },
    };
    const ui = bridgeFromWire(raw);
    const next = write({ ...ui, tapRetention: { days: 7, maxMessages: null } }, raw);
    expect(next.tap).toEqual({
      framer: "hl7_mllp",
      record_messages: true,
      max_message_bytes: 4096,
      retention: { max_age_days: 7 },
    });
    expect(write(ui, raw)).toEqual(raw);
  });

  it("removes a limit by writing null, and the count limit by itself", () => {
    const ui = bridgeFromWire(RAW);
    expect(
      write({ ...ui, tapRetention: { days: null, maxMessages: 2000 } }).tap?.retention,
    ).toEqual({
      max_age_days: null,
      max_messages: 2000,
    });
  });

  it("leaves the default out of a new bridge, and writes what differs", () => {
    const ui: BridgeChannelConfig = {
      ...bridgeFromWire(RAW),
      tapRetention: { days: 30, maxMessages: null },
    };
    expect(write(ui, undefined).tap).toBeUndefined();
    const changed = write({ ...ui, tapRetention: { days: 14, maxMessages: 10_000 } }, undefined);
    expect(changed.tap).toEqual({ retention: { max_age_days: 14, max_messages: 10_000 } });
    expect(documentProblems({ schema_version: 1, channels: [changed] })).toEqual([]);
  });
});
