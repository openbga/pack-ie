import { describe, expect, it } from "vitest";
import { documentFromWire, documentToWire } from "./configDocument";
import { IDS, richConfig } from "./test/configFixtures";
import { AU, destination, message, withNew } from "./test/newItems";

describe("new channels are written sparse", () => {
  it("a bridge of defaults is its ids", () => {
    const { channels } = withNew(
      [],
      [
        {
          kind: "bridge",
          id: "new-b",
          label: "B",
          enabled: true,
          endpointAId: IDS.serial,
          endpointBId: IDS.client,
          pairing: "parallel",
          pairingReceiver: null,
          bufferBytes: 10 * 1024 * 1024,
          retry: { initialMs: 1000, maxMs: 60_000, maxAttempts: null },
          tapFramer: "none",
          tapRetention: { days: 30, maxMessages: null },
        },
      ],
    );
    expect(channels).toEqual([
      {
        channel_id: "new-b",
        label: "B",
        kind: "bridge",
        endpoint_a: IDS.serial,
        endpoint_b: IDS.client,
      },
    ]);
  });

  it("a bridge writes pairing, buffer, retry and tap where they differ", () => {
    const { channels, issues } = withNew(
      [],
      [
        {
          kind: "bridge",
          id: "new-b",
          label: "B",
          enabled: false,
          endpointAId: IDS.serial,
          endpointBId: IDS.client,
          pairing: "receiver_then_sender",
          pairingReceiver: "b",
          bufferBytes: 1024,
          retry: { initialMs: 500, maxMs: 9000, maxAttempts: null },
          tapFramer: "au",
          tapRetention: { days: 30, maxMessages: null },
        },
      ],
    );
    expect(issues).toEqual([]);
    expect(channels).toEqual([
      {
        channel_id: "new-b",
        label: "B",
        kind: "bridge",
        enabled: false,
        endpoint_a: IDS.serial,
        endpoint_b: IDS.client,
        connection: { connect_strategy: "receiver_then_sender", receiver: "b" },
        buffer: { max_bytes: 1024 },
        retry: { initial_delay_ms: 500, max_delay_ms: 9000 },
        tap: { framer: "au" },
      },
    ]);
  });

  it("a message channel of defaults is a source and its destinations", () => {
    const { channels, issues } = withNew([], [message()]);
    expect(issues).toEqual([]);
    expect(channels).toEqual([
      {
        channel_id: "new-m",
        label: "M",
        kind: "message",
        source: { endpoint: IDS.listener, protocol: "astm" },
        destinations: [
          { destination_id: "d1", label: "LIS", endpoint: IDS.client, protocol: "hl7_mllp" },
        ],
      },
    ]);
  });

  it("a message channel writes filters, connectors, mappings and retention", () => {
    const { channels, issues } = withNew(
      [],
      [
        message({
          enabled: false,
          source: {
            id: "source",
            endpointId: IDS.auListener,
            protocol: "au",
            settings: { au: AU },
            duplicateDetection: false,
          },
          filters: [{ id: "f", path: "R.3", operator: "in", value: "a, b,, c", ignoreCase: true }],
          destinations: [
            destination({
              settings: { ackMode: "enhanced" },
              filters: [
                { id: "g", path: "OBX.3", operator: "exists", value: "", ignoreCase: false },
              ],
              mappings: [
                { id: "m0", op: "constant", value: "X", to: "H.5" },
                { id: "m1", op: "sequence", to: "L.1", start: 1 },
              ],
              codeTables: {},
              retry: { initialMs: 1000, maxMs: 60_000, maxAttempts: 10 },
            }),
            destination({
              id: "d2",
              label: "CSV",
              endpointId: IDS.folderOut,
              protocol: "delimited_file",
              settings: { templateJson: '{"delimiter":";","columns":[{"name":"a"}]}' },
              retry: { initialMs: 2000, maxMs: 30_000, maxAttempts: 4 },
            }),
            destination({
              id: "d3",
              label: "Archive",
              endpointId: IDS.client,
              protocol: "astm",
              settings: { astmRole: "instrument" },
              enabled: false,
              codeTables: { t: { a: "b" } },
            }),
          ],
          retention: { days: null, maxMessages: 500 },
        }),
      ],
    );
    expect(issues).toEqual([]);
    expect(channels).toEqual([
      {
        channel_id: "new-m",
        label: "M",
        kind: "message",
        enabled: false,
        source: {
          endpoint: IDS.auListener,
          protocol: "au",
          settings: {
            profile: "au400",
            framing: {
              start: [2],
              end: [3],
              bcc: true,
              use_etb: false,
              max_wire_len: 256,
              class: "a",
            },
            format: {
              unit_no: false,
              rack_digits: 0,
              sample_id_digits: 6,
              sex: false,
              age: false,
              month: false,
              patient_info_digits: [0, 0, 0, 0, 0, 0],
              result_digits: 6,
            },
            frame_timeout_ms: 1000,
            block_timeout_ms: 1000,
          },
          duplicate_detection: false,
        },
        filters: [{ path: "R.3", op: "in", value: ["a", "b", "c"], ignore_case: true }],
        destinations: [
          {
            destination_id: "d1",
            label: "LIS",
            endpoint: IDS.client,
            protocol: "hl7_mllp",
            settings: { ack_mode: "enhanced" },
            filters: [{ path: "OBX.3", op: "exists" }],
            mapping: {
              rules: [
                { op: "constant", value: "X", to: "H.5" },
                { op: "sequence", to: "L.1" },
              ],
            },
            delivery: { max_attempts: 10 },
          },
          {
            destination_id: "d2",
            label: "CSV",
            endpoint: IDS.folderOut,
            protocol: "delimited_file",
            settings: { delimiter: ";", columns: [{ name: "a" }] },
            delivery: {
              max_attempts: 4,
              retry: { initial_delay_ms: 2000, max_delay_ms: 30_000 },
            },
          },
          {
            destination_id: "d3",
            label: "Archive",
            enabled: false,
            endpoint: IDS.client,
            protocol: "astm",
            settings: { role: "instrument" },
            mapping: { tables: { t: { a: "b" } }, rules: [] },
          },
        ],
        retention: { max_age_days: null, max_messages: 500 },
      },
    ]);
  });

  it("a new destination that matches its channel's delivery policy has no overrides of its own", () => {
    const raw = richConfig();
    const doc = documentFromWire(raw);
    const channel = doc.channels[1];
    if (channel?.kind !== "message") throw new Error("fixture");
    // The channel's policy: 7 attempts, 2.5 s first delay, 90 s longest delay.
    channel.destinations.push(
      destination({ id: "same", retry: { initialMs: 2500, maxMs: 90_000, maxAttempts: 7 } }),
      destination({ id: "other", retry: { initialMs: 2500, maxMs: 90_000, maxAttempts: 2 } }),
      destination({ id: "slower", retry: { initialMs: 5000, maxMs: 90_000, maxAttempts: 7 } }),
    );
    const { document } = documentToWire(doc, raw);
    const written = document.channels?.[1];
    if (written?.kind !== "message") throw new Error("fixture");
    const [same, other, slower] = written.destinations.slice(3);
    expect(same).not.toHaveProperty("delivery");
    expect(other?.delivery).toEqual({ max_attempts: 2 });
    expect(slower?.delivery).toEqual({ retry: { initial_delay_ms: 5000, max_delay_ms: 90_000 } });
  });

  it("a new delimited connector without a usable template is reported", () => {
    const { issues } = withNew(
      [],
      [
        message({
          source: {
            id: "source",
            endpointId: IDS.folder,
            protocol: "delimited_file",
            settings: { templateJson: "" },
            duplicateDetection: true,
          },
        }),
      ],
    );
    expect(issues).toEqual([
      {
        severity: "error",
        path: "channels[3].source.settings.templateJson",
        message: "A column template is required.",
      },
    ]);
  });
});
