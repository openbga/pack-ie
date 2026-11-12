import { describe, expect, it } from "vitest";
import type { ConfigDocument } from "../types";
import { documentToWire } from "./configDocument";
import { AU, base, destination, message, sftp, withNew } from "./test/newItems";
import { documentProblems } from "./test/specSchema";

describe("new endpoints are written sparse", () => {
  it("serial, tcp and folder endpoints carry what the engine needs and what differs from its defaults", () => {
    const { endpoints } = withNew(
      [
        {
          ...base,
          kind: "serial",
          port: "COM9",
          baudRate: 9600,
          dataBits: 8,
          parity: "none",
          stopBits: 1,
          flowControl: "none",
        },
        {
          ...base,
          id: "new-2",
          kind: "tcp_client",
          host: "10.0.0.2",
          port: 3001,
          connectTimeoutMs: 5000,
          idleTimeoutSeconds: 0,
          keepalive: true,
        },
        {
          ...base,
          id: "new-3",
          enabled: false,
          kind: "tcp_listener",
          bindAddress: "127.0.0.1",
          port: 5100,
          allowlist: [],
          allowAnyPeer: false,
          idleTimeoutSeconds: 0,
          keepalive: false,
        },
        {
          ...base,
          id: "new-4",
          kind: "folder",
          path: "/in",
          mode: "watch",
          pollIntervalSec: 5,
          filePattern: "*",
        },
      ],
      [],
    );
    expect(endpoints).toEqual([
      {
        endpoint_id: "new-1",
        label: "New",
        kind: "serial",
        settings: { port_name: "COM9", baud_rate: 9600 },
      },
      {
        endpoint_id: "new-2",
        label: "New",
        kind: "tcp_client",
        settings: { host: "10.0.0.2", port: 3001 },
      },
      {
        endpoint_id: "new-3",
        label: "New",
        enabled: false,
        kind: "tcp_listener",
        settings: { bind_address: "127.0.0.1", port: 5100, keep_alive: false },
      },
      {
        endpoint_id: "new-4",
        label: "New",
        kind: "folder",
        settings: { path: "/in", poll_interval_seconds: 5 },
      },
    ]);
  });

  it("writes every setting that differs from the default", () => {
    const { endpoints } = withNew(
      [
        {
          ...base,
          kind: "serial",
          port: "COM9",
          baudRate: 115_200,
          dataBits: 7,
          parity: "odd",
          stopBits: 2,
          flowControl: "xon_xoff",
        },
        {
          ...base,
          id: "new-2",
          kind: "tcp_listener",
          bindAddress: "10.0.0.5",
          port: 5100,
          allowlist: ["10.0.0.0/8"],
          allowAnyPeer: true,
          idleTimeoutSeconds: 60,
          keepalive: true,
        },
        {
          ...base,
          id: "new-3",
          kind: "folder",
          path: "/in",
          mode: "poll",
          pollIntervalSec: 30,
          filePattern: "*.csv",
        },
      ],
      [],
    );
    expect(endpoints.map((e) => e.settings)).toEqual([
      {
        port_name: "COM9",
        baud_rate: 115_200,
        data_bits: 7,
        parity: "odd",
        stop_bits: "two",
        flow_control: "xon_xoff",
      },
      {
        bind_address: "10.0.0.5",
        port: 5100,
        allowed_peer_cidrs: ["10.0.0.0/8"],
        allow_any_peer: true,
        idle_timeout_seconds: 60,
      },
      { path: "/in", mode: "poll", file_pattern: "*.csv" },
    ]);
  });

  it("SFTP endpoints write their authentication as secret references", () => {
    const { endpoints } = withNew(
      [
        sftp,
        {
          ...sftp,
          id: "new-2",
          authMethod: "private_key",
          secretName: "key",
          passphraseSecretName: "phrase",
        },
        { ...sftp, id: "new-3", authMethod: "private_key", secretName: "key", port: 2022 },
      ],
      [],
    );
    expect(endpoints.map((e) => (e.kind === "sftp" ? e.settings.auth : null))).toEqual([
      { method: "password", password: { secret: "pw" } },
      { method: "key", private_key: { secret: "key" }, passphrase: { secret: "phrase" } },
      { method: "key", private_key: { secret: "key" } },
    ]);
    expect(endpoints[0]?.settings).toEqual({
      host: "h",
      username: "u",
      auth: { method: "password", password: { secret: "pw" } },
      host_key_fingerprint: "SHA256:abcdefghij",
      remote_directory: "/in",
    });
    expect(endpoints[2]?.settings).toMatchObject({ port: 2022 });
  });

  it("SFTP endpoints write the file name pattern only when it is not the default", () => {
    const { endpoints } = withNew([sftp, { ...sftp, id: "new-2", filePattern: "*.csv" }], []);
    expect(endpoints[0]?.settings).not.toHaveProperty("file_pattern");
    expect(endpoints[1]?.settings).toMatchObject({ file_pattern: "*.csv" });
  });
});

describe("a document without an original is sparse", () => {
  it("has the schema version and what was added", () => {
    const doc: ConfigDocument = {
      version: 0,
      endpoints: [],
      channels: [],
      alertThresholds: {
        endpointDownSeconds: 60,
        queueDepthWarning: 1000,
        bufferPressurePercent: 80,
        diskFreeMinMib: 1024,
      },
    };
    expect(documentToWire(doc).document).toEqual({ schema_version: 1 });
    doc.alertThresholds.endpointDownSeconds = 30;
    doc.alertThresholds.diskFreeMinMib = 2048;
    expect(documentToWire(doc).document.alerts).toEqual({
      endpoint_down_seconds: 30,
      disk_low_megabytes: 2048,
    });
  });
});

describe("everything written has the shape the engine accepts", () => {
  it("covers every endpoint kind and both channel kinds", () => {
    const { document, issues } = withNew(
      [
        {
          ...base,
          id: "n1",
          kind: "serial",
          port: "COM9",
          baudRate: 9600,
          dataBits: 7,
          parity: "even",
          stopBits: 2,
          flowControl: "rts_cts",
        },
        {
          ...base,
          id: "n2",
          kind: "tcp_client",
          host: "h",
          port: 1,
          connectTimeoutMs: 1,
          idleTimeoutSeconds: 5,
          keepalive: false,
        },
        {
          ...base,
          id: "n3",
          kind: "tcp_listener",
          bindAddress: "::1",
          port: 1,
          allowlist: ["::1/128"],
          allowAnyPeer: true,
          idleTimeoutSeconds: 3,
          keepalive: false,
        },
        {
          ...base,
          id: "n4",
          kind: "folder",
          path: "/in",
          mode: "poll",
          pollIntervalSec: 2,
          filePattern: "*.csv",
        },
        { ...sftp, id: "n5" },
        { ...sftp, id: "n6", authMethod: "private_key", passphraseSecretName: "p" },
      ],
      [
        {
          kind: "bridge",
          id: "nb",
          label: "B",
          enabled: false,
          endpointAId: "n1",
          endpointBId: "n2",
          pairing: "wait_for_listener_then_pair",
          pairingReceiver: null,
          bufferBytes: 1,
          retry: { initialMs: 1, maxMs: 2, maxAttempts: null },
          tapFramer: "hl7_mllp",
          tapRetention: { days: 30, maxMessages: null },
        },
        message({
          id: "nm",
          source: {
            id: "source",
            endpointId: "n3",
            protocol: "au",
            settings: { au: AU },
            duplicateDetection: false,
          },
          filters: [{ id: "f", path: "R.3", operator: "equals", value: "x", ignoreCase: false }],
          destinations: [
            destination({
              endpointId: "n2",
              mappings: [
                { id: "a", op: "copy", from: "A", to: "B", default: "d", required: true },
                {
                  id: "b",
                  op: "when",
                  condition: { path: "A", operator: "in", value: "x, y", ignoreCase: false },
                  // biome-ignore lint/suspicious/noThenProperty: `then` is the rule's own field
                  then: [{ id: "c", op: "constant", value: "1", to: "C" }],
                  else: [{ id: "d", op: "constant", value: "2", to: "C" }],
                },
                {
                  id: "e",
                  op: "for_each",
                  over: "R",
                  into: "OBX",
                  rules: [{ id: "f", op: "sequence", to: "$.1", start: 3 }],
                },
              ],
              codeTables: { a: { b: "c" } },
            }),
          ],
          retention: { days: 1, maxMessages: 2000 },
        }),
      ],
    );
    expect(issues).toEqual([]);
    expect(documentProblems(document)).toEqual([]);
  });
});
