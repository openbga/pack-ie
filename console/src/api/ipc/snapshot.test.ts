import { describe, expect, it } from "vitest";
import type { Alert } from "@/ipc/generated";
import { alertFromIpc } from "./alerts";
import { destinationFrom } from "./destinations";
import { snapshotFromIpc } from "./snapshot";
import { transportSummary } from "./summary";
import { IDS, richConfig } from "./test/configFixtures";
import { HL7_DESTINATION_STATUS } from "./test/hl7Destination";
import {
  destinationSnapshot,
  endpointSnapshot,
  engineAlert,
  engineSnapshot,
  messagePart,
} from "./test/snapshotFixtures";
import type { WireEndpoint } from "./wire";

const config = richConfig();

describe("node", () => {
  it.each([
    ["healthy", "running"],
    ["degraded", "degraded"],
    ["stopped", "stopping"],
  ] as const)("%s reads %s", (state, expected) => {
    const raw = engineSnapshot();
    raw.node.node_state = state;
    const mapped = snapshotFromIpc(raw, config);
    expect(mapped.node).toEqual({
      nodeId: "node-1",
      nodeName: "Lab 1",
      version: "0.1.0",
      startedAt: "2026-11-06T10:00:00.000Z",
      state: expected,
      hosting: "service",
      configVersion: 7,
      pendingConfigVersion: null,
      store: {
        state: "ok",
        messageCount: 120,
        databaseBytes: 4096,
        diskFreeBytes: 1_000_000,
        diskTotalBytes: 8_000_000,
      },
      totals: {
        deliveriesUnresolved: 6,
        deliveriesQueued: 201,
        alertsWarning: 1,
        alertsCritical: 0,
      },
    });
    expect(mapped.takenAt).toBe("2026-11-06T12:00:10.000Z");
  });

  it("reports an apply in progress", () => {
    const raw = engineSnapshot();
    raw.node.config = { active_version: 7, pending_version: 8, last_apply: null };
    expect(snapshotFromIpc(raw, config).node).toMatchObject({
      configVersion: 7,
      pendingConfigVersion: 8,
    });
  });

  it("keeps the store figures the engine does not report null", () => {
    const raw = engineSnapshot();
    raw.node.hosting_mode = "console";
    raw.node.totals.messages_stored = null;
    raw.node.store = {
      state: "not_configured",
      size_bytes: null,
      free_disk_bytes: null,
      total_disk_bytes: null,
    };
    raw.node.config = { active_version: null, pending_version: null, last_apply: null };
    expect(snapshotFromIpc(raw, null).node).toMatchObject({
      hosting: "console",
      store: {
        state: "not_configured",
        messageCount: null,
        databaseBytes: null,
        diskFreeBytes: null,
        diskTotalBytes: null,
      },
      configVersion: 0,
      pendingConfigVersion: null,
    });
  });

  it("counts a delivery total the engine does not report as 0", () => {
    const raw = engineSnapshot();
    raw.node.totals.deliveries_unresolved = null;
    raw.node.totals.deliveries_queued = null;
    expect(snapshotFromIpc(raw, null).node.totals).toEqual({
      deliveriesUnresolved: 0,
      deliveriesQueued: 0,
      alertsWarning: 1,
      alertsCritical: 0,
    });
  });
});

describe("destinations", () => {
  it("reads the label, the queue, the unresolved counts, the counters and the times", () => {
    const [hl7, csv] = messagePart().destinations;
    if (!hl7 || !csv) throw new Error("fixture");
    expect(destinationFrom(hl7)).toEqual(HL7_DESTINATION_STATUS);
    expect(destinationFrom(csv)).toMatchObject({
      label: "Export CSV",
      enabled: false,
      protocol: "delimited_file",
      queued: 0,
      sending: 0,
      oldestQueuedAt: "2026-11-06T11:00:00.000Z",
      lastAcknowledgedAt: null,
      lastAttemptAt: null,
      nextAttemptAt: null,
      lastError: null,
      unresolved: { rejected: 0, failed: 3, timed_out: 0, outcome_unknown: 1 },
    });
  });

  it("keeps what the engine does not say as null, never as zero", () => {
    const quiet = destinationFrom(destinationSnapshot("d", IDS.client));
    expect(quiet).toMatchObject({
      oldestQueuedAt: null,
      nextAttemptAt: null,
      lastAttemptAt: null,
      lastAcknowledgedAt: null,
    });
  });

  it("has no error when the engine reports none", () => {
    expect(destinationFrom(destinationSnapshot("d", IDS.client)).lastError).toBeNull();
  });
});

describe("endpoints", () => {
  it("summarizes with the engine's description, else the document, else the snapshot", () => {
    const raw = engineSnapshot();
    raw.endpoints = [
      endpointSnapshot(IDS.serial),
      endpointSnapshot(IDS.serial, { settings_summary: "" }),
      endpointSnapshot("unknown", {
        kind: "tcp_client",
        settings_summary: "",
        peer: "10.0.0.9:3000",
      }),
      endpointSnapshot("other", { kind: "folder", settings_summary: "" }),
    ];
    expect(snapshotFromIpc(raw, config).endpoints.map((e) => e.summary)).toEqual([
      "Analyzer (COM3) summary",
      "COM3 · 19200 7E2",
      "10.0.0.9:3000",
      "folder",
    ]);
    expect(snapshotFromIpc(raw, null).endpoints.map((e) => e.summary)).toEqual([
      "Analyzer (COM3) summary",
      "serial",
      "10.0.0.9:3000",
      "folder",
    ]);
  });

  it("maps state, counters and links", () => {
    const raw = engineSnapshot();
    raw.endpoints = [
      endpointSnapshot(IDS.client, {
        state: "retrying",
        consecutive_failures: 4,
        retry_count: 5,
        next_retry_at: "2026-11-06T12:00:30.000Z",
        held_by: IDS.bridge,
        peer: "10.0.0.20:3001",
        channels: [IDS.bridge],
        last_error: {
          at: "2026-11-06T11:59:00.000Z",
          summary: "refused.",
          next_action: "Retrying in 4s.",
        },
      }),
    ];
    expect(snapshotFromIpc(raw, config).endpoints[0]).toEqual({
      id: IDS.client,
      label: "LIS HL7",
      kind: "tcp_client",
      enabled: true,
      state: "retrying",
      summary: "LIS HL7 summary",
      counters: { bytesIn: 10, bytesOut: 20, messagesIn: 1, messagesOut: 2, retries: 5 },
      lastActivityAt: "2026-11-06T12:00:05.000Z",
      stateSince: "2026-11-06T11:00:00.000Z",
      lastError: {
        at: "2026-11-06T11:59:00.000Z",
        summary: "refused.",
        nextAction: "Retrying in 4s.",
      },
      channelIds: [IDS.bridge],
      heldBy: IDS.bridge,
      activePeer: "10.0.0.20:3001",
      consecutiveFailureCount: 4,
      nextRetryAt: "2026-11-06T12:00:30.000Z",
    });
  });
});

describe("transport summaries", () => {
  const expected = [
    "COM3 · 19200 7E2",
    "listen 0.0.0.0:5200",
    "listen 127.0.0.1:5300",
    "tcp://lis.example:6661",
    "/data/in",
    "sftp://lab@sftp.example:2222/out",
    "/data/out",
  ];

  it.each((config.endpoints ?? []).map((e, i) => [e.label, i] as const))("%s", (_label, index) => {
    const endpoint = config.endpoints?.[index];
    expect(endpoint && transportSummary(endpoint)).toBe(expected[index]);
  });

  it("names a virtual serial link", () => {
    const endpoint: WireEndpoint = {
      endpoint_id: "a",
      label: "a",
      kind: "serial",
      settings: { port_name: "virtual", baud_rate: 9600, virtual_link: "/tmp/packie-a" },
    };
    expect(transportSummary(endpoint)).toBe("virtual /tmp/packie-a · 9600 8N1");
  });

  it("brackets IPv6 hosts and adds the leading slash of SFTP paths", () => {
    expect(
      transportSummary({
        endpoint_id: "a",
        label: "a",
        kind: "tcp_listener",
        settings: { bind_address: "::1", port: 80 },
      }),
    ).toBe("listen [::1]:80");
    expect(
      transportSummary({
        endpoint_id: "a",
        label: "a",
        kind: "sftp",
        settings: {
          host: "h",
          username: "u",
          remote_directory: "in",
          host_key_fingerprint: "SHA256:x",
          auth: { method: "password", password: { secret: "p" } },
        },
      }),
    ).toBe("sftp://u@h/in");
  });
});

describe("alerts", () => {
  it("maps fields and sorts critical, warning (stable within a severity)", () => {
    const raw = engineSnapshot();
    raw.alerts = [
      engineAlert("a", "warning"),
      engineAlert("b", "critical"),
      engineAlert("c", "warning"),
      engineAlert("d", "critical"),
    ];
    const alerts = snapshotFromIpc(raw, config).openAlerts;
    expect(alerts.map((a) => a.id)).toEqual(["b", "d", "a", "c"]);
    expect(alerts[0]).toEqual({
      id: "b",
      severity: "critical",
      kind: "endpoint_down",
      title: "summary b",
      detail: "count: 2, peer: 10.0.0.9",
      channelId: null,
      endpointId: IDS.client,
      firstSeenAt: "2026-11-06T11:00:00.000Z",
      lastSeenAt: "2026-11-06T11:30:00.000Z",
      occurrences: 3,
      state: "open",
      clearedAt: null,
      nextAction: "Check the cable",
    });
  });

  it("reads a cleared alert", () => {
    expect(
      alertFromIpc(
        engineAlert("x", "warning", {
          state: "cleared",
          cleared_at: "2026-11-06T11:45:00.000Z",
          details: {},
          occurrences: 1,
        }),
      ),
    ).toMatchObject({
      state: "cleared",
      clearedAt: "2026-11-06T11:45:00.000Z",
      detail: "",
      occurrences: 1,
    });
  });

  it("keeps every alert kind the engine can raise", () => {
    const kinds: Alert["code"][] = [
      "endpoint_down",
      "no_peer_connected",
      "peer_rejected",
      "bridge_buffer_pressure",
      "message_store_unavailable",
      "disk_low",
      "delivery_needs_attention",
      "transform_failed",
      "delivery_backlog",
      "invalid_messages",
      "config_rejected",
      "config_apply_failed",
    ];
    for (const code of kinds)
      expect(alertFromIpc(engineAlert("a", "warning", { code })).kind).toBe(code);
  });
});
