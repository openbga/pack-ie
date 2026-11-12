import { describe, expect, it } from "vitest";
import type { ConfigDocument } from "../types";
import { documentFromWire, documentToWire } from "./configDocument";
import { IDS, richConfig } from "./test/configFixtures";
import { bridge, destination, diffPaths, endpoint, message } from "./test/documentItems";
import type { WireConfig } from "./wire";

const write = documentToWire;

describe("editing one modeled field changes only its engine field", () => {
  const edits: [string, (doc: ConfigDocument) => void, string[]][] = [
    [
      "serial baud rate",
      (d) => {
        endpoint(d, IDS.serial, "serial").baudRate = 38_400;
      },
      ["endpoints[0].settings.baud_rate"],
    ],
    [
      "serial stop bits and parity",
      (d) => {
        const e = endpoint(d, IDS.serial, "serial");
        e.stopBits = 1;
        e.parity = "odd";
      },
      ["endpoints[0].settings.stop_bits", "endpoints[0].settings.parity"],
    ],
    [
      "endpoint label",
      (d) => {
        const e = d.endpoints[3];
        if (e) e.label = "Renamed";
      },
      ["endpoints[3].label"],
    ],
    [
      "endpoint enabled",
      (d) => {
        const e = d.endpoints[4];
        if (e) e.enabled = true;
      },
      ["endpoints[4].enabled"],
    ],
    [
      "listener allowlist",
      (d) => {
        endpoint(d, IDS.listener, "tcp_listener").allowlist.push("172.16.0.0/12");
      },
      ["endpoints[1].settings.allowed_peer_cidrs"],
    ],
    [
      "listener keep-alive on keeps its tuning",
      (d) => {
        endpoint(d, IDS.listener, "tcp_listener").keepalive = true;
      },
      ["endpoints[1].settings.keep_alive"],
    ],
    [
      "client idle timeout",
      (d) => {
        endpoint(d, IDS.client, "tcp_client").idleTimeoutSeconds = 0;
      },
      ["endpoints[3].settings.idle_timeout_seconds"],
    ],
    [
      "folder poll interval",
      (d) => {
        endpoint(d, IDS.folder, "folder").pollIntervalSec = 20;
      },
      ["endpoints[4].settings.poll_interval_seconds"],
    ],
    [
      "folder pattern",
      (d) => {
        endpoint(d, IDS.folder, "folder").filePattern = "*.hl7";
      },
      ["endpoints[4].settings.file_pattern"],
    ],
    [
      "sftp pattern",
      (d) => {
        endpoint(d, IDS.sftp, "sftp").filePattern = "*.csv";
      },
      ["endpoints[5].settings.file_pattern"],
    ],
    [
      "sftp passphrase removed",
      (d) => {
        endpoint(d, IDS.sftp, "sftp").passphraseSecretName = "";
      },
      ["endpoints[5].settings.auth.passphrase"],
    ],
    [
      "sftp secret renamed",
      (d) => {
        endpoint(d, IDS.sftp, "sftp").secretName = "other-key";
      },
      ["endpoints[5].settings.auth.private_key.secret"],
    ],
    [
      "sftp switched to password",
      (d) => {
        const e = endpoint(d, IDS.sftp, "sftp");
        e.authMethod = "password";
        e.passphraseSecretName = "";
      },
      [
        "endpoints[5].settings.auth.method",
        "endpoints[5].settings.auth.private_key",
        "endpoints[5].settings.auth.passphrase",
        "endpoints[5].settings.auth.password",
      ],
    ],
    [
      "bridge buffer",
      (d) => {
        bridge(d).bufferBytes = 4_194_304;
      },
      ["channels[0].buffer.max_bytes"],
    ],
    [
      "bridge pairing",
      (d) => {
        const b = bridge(d);
        b.pairing = "parallel";
        b.pairingReceiver = null;
      },
      ["channels[0].connection.connect_strategy", "channels[0].connection.receiver"],
    ],
    [
      "bridge tap retention: age limit",
      (d) => {
        bridge(d).tapRetention.days = 7;
      },
      ["channels[0].tap.retention"],
    ],
    [
      "bridge tap retention: count limit",
      (d) => {
        bridge(d).tapRetention.maxMessages = 5000;
      },
      ["channels[0].tap.retention"],
    ],
    [
      "bridge tap off",
      (d) => {
        bridge(d).tapFramer = "none";
      },
      ["channels[0].tap.framer"],
    ],
    [
      "bridge retry delays",
      (d) => {
        bridge(d).retry.initialMs = 500;
        bridge(d).retry.maxMs = 10_000;
      },
      ["channels[0].retry.initial_delay_ms", "channels[0].retry.max_delay_ms"],
    ],
    [
      "message channel label",
      (d) => {
        message(d).label = "Chem";
      },
      ["channels[1].label"],
    ],
    [
      "message channel enabled",
      (d) => {
        message(d, IDS.au).enabled = true;
      },
      ["channels[2].enabled"],
    ],
    [
      "retention without age limit",
      (d) => {
        message(d).retention.days = null;
      },
      ["channels[1].retention.max_age_days"],
    ],
    [
      "source duplicate detection",
      (d) => {
        message(d).source.duplicateDetection = true;
      },
      ["channels[1].source.duplicate_detection"],
    ],
    [
      "source link role",
      (d) => {
        message(d).source.settings.astmRole = "computer";
      },
      ["channels[1].source.settings.role"],
    ],
    [
      "source endpoint",
      (d) => {
        message(d).source.endpointId = IDS.client;
      },
      ["channels[1].source.endpoint"],
    ],
    [
      "channel filter value",
      (d) => {
        const f = message(d).filters[0];
        if (f && "value" in f) f.value = "M";
      },
      ["channels[1].filters[0].value"],
    ],
    [
      "destination label",
      (d) => {
        destination(d, 0).label = "Renamed";
      },
      ["channels[1].destinations[0].label"],
    ],
    [
      "destination enabled",
      (d) => {
        destination(d, 1).enabled = true;
      },
      ["channels[1].destinations[1].enabled"],
    ],
    [
      "destination ack mode",
      (d) => {
        destination(d, 0).settings.ackMode = "original";
      },
      ["channels[1].destinations[0].settings.ack_mode"],
    ],
    [
      "destination endpoint",
      (d) => {
        destination(d, 1).endpointId = IDS.folder;
      },
      ["channels[1].destinations[1].endpoint"],
    ],
    [
      "destination filter",
      (d) => {
        const f = destination(d, 0).filters[0];
        if (f && "value" in f) f.value = "X, Z";
      },
      ["channels[1].destinations[0].filters[0].value"],
    ],
    [
      "destination retry attempts",
      (d) => {
        destination(d, 0).retry.maxAttempts = 3;
      },
      ["channels[1].destinations[0].delivery.max_attempts"],
    ],
    [
      "destination first retry delay",
      (d) => {
        destination(d, 0).retry.initialMs = 900;
      },
      ["channels[1].destinations[0].delivery.retry.initial_delay_ms"],
    ],
    [
      "destination retry that was the channel's",
      (d) => {
        destination(d, 1).retry.maxAttempts = 3;
      },
      ["channels[1].destinations[1].delivery"],
    ],
    [
      "delimited settings",
      (d) => {
        const dest = destination(d, 1);
        const template = JSON.parse(dest.settings.templateJson ?? "{}");
        template.delimiter = ",";
        dest.settings.templateJson = JSON.stringify(template);
      },
      ["channels[1].destinations[1].settings.delimiter"],
    ],
    [
      "mapping rule target",
      (d) => {
        const rule = destination(d, 0).mappings[0];
        if (rule?.op === "copy") rule.to = "R.9";
      },
      ["channels[1].destinations[0].mapping.rules[0].to"],
    ],
    [
      "mapping rule inside a loop",
      (d) => {
        const loop = destination(d, 0).mappings[7];
        const copy = loop?.op === "for_each" ? loop.rules[1] : undefined;
        if (copy?.op === "copy") copy.from = "$.9";
      },
      ["channels[1].destinations[0].mapping.rules[7].rules[1].from"],
    ],
    [
      "code table entry",
      (d) => {
        const sex = destination(d, 0).codeTables.sex;
        if (sex) sex.M = "man";
      },
      ["channels[1].destinations[0].mapping.tables.sex.M"],
    ],
    [
      "mapping removed",
      (d) => {
        const dest = destination(d, 0);
        dest.mappings = [];
        dest.codeTables = {};
      },
      ["channels[1].destinations[0].mapping"],
    ],
    [
      "mapping given to a pass-through destination",
      (d) => {
        destination(d, 2).mappings = [{ id: "n", op: "constant", value: "X", to: "H.5" }];
      },
      ["channels[1].destinations[2].mapping"],
    ],
    [
      "AU framing",
      (d) => {
        const au = message(d, IDS.au).source.settings.au;
        if (au) au.bcc = false;
      },
      ["channels[2].source.settings.framing.bcc"],
    ],
    [
      "AU timing",
      (d) => {
        const au = message(d, IDS.au).source.settings.au;
        if (au) au.frameTimeoutMs = 4000;
      },
      ["channels[2].source.settings.frame_timeout_ms"],
    ],
    [
      "alert threshold",
      (d) => {
        d.alertThresholds.queueDepthWarning = 250;
      },
      ["alerts.delivery_backlog_count"],
    ],
  ];

  it.each(edits)("%s", (_name, edit, expected) => {
    const raw = richConfig();
    const doc = documentFromWire(raw);
    edit(doc);
    const converted = write(doc, raw);
    expect(converted.issues).toEqual([]);
    expect(diffPaths(raw, converted.document).sort()).toEqual([...expected].sort());
  });

  it("does not modify the document it started from", () => {
    const raw = richConfig();
    const doc = documentFromWire(raw);
    endpoint(doc, IDS.serial, "serial").baudRate = 1200;
    destination(doc, 0).filters = [];
    write(doc, raw);
    expect(raw).toEqual(richConfig());
  });

  it("does not share objects with the document it started from", () => {
    const raw = richConfig();
    const { document } = write(documentFromWire(raw), raw);
    const channel = document.channels?.[1];
    const original = raw.channels?.[1];
    if (channel?.kind !== "message" || original?.kind !== "message") throw new Error("fixture");
    expect(channel.destinations[0]?.mapping).not.toBe(original.destinations[0]?.mapping);
    expect(channel.filters).not.toBe(original.filters);
  });

  it("echoes the version, schema version and time of the document it started from", () => {
    const raw = richConfig();
    const { document } = write(documentFromWire(raw), raw);
    expect(document.config_version).toBe(7);
    expect(document.schema_version).toBe(1);
    expect(document.generated_at).toBe(raw.generated_at);
    expect(document.diagnostics).toEqual({ snapshot_interval_seconds: 5 });
  });

  it("keeps the other alert thresholds when one changes", () => {
    const raw = richConfig();
    const doc = documentFromWire(raw);
    doc.alertThresholds.endpointDownSeconds = 30;
    const { document } = write(doc, raw);
    expect(document.alerts).toEqual({ ...raw.alerts, endpoint_down_seconds: 30 });
  });

  it("writes a document that has no schema version or alerts without adding them", () => {
    const raw: WireConfig = { endpoints: [], channels: [] };
    expect(write(documentFromWire(raw), raw).document).toEqual({
      schema_version: 1,
      endpoints: [],
      channels: [],
    });
  });
});
