import { describe, expect, it } from "vitest";
import { IDS } from "./test/configFixtures";
import { CONNECTED, engine } from "./test/ipcEngine";

describe("settings", () => {
  const bootstrap = {
    schema_version: 1,
    node: { node_id: "node-9", display_name: "Lab 9", environment: "prod" },
    paths: { state_directory: "/var/lib/packie", log_directory: "/var/log/packie" },
    logging: { level: "info", retention_days: 45, ring_buffer_events: 5000 },
  };

  it("reads the bootstrap document and the connection", async () => {
    const { bridge, api } = engine();
    bridge.command("read_bootstrap", () => bootstrap);
    await expect(api.getEngineSettings()).resolves.toEqual({
      nodeId: "node-9",
      nodeName: "Lab 9",
      stateDir: "/var/lib/packie",
      logDir: "/var/log/packie",
      ipcPath: "/run/packie.sock",
      hosting: "service",
      logRetentionDays: 45,
      logRingSize: 5000,
      notifications: null,
    });
  });

  it("reports the bootstrap file the console resolved the engine from", async () => {
    const { bridge, api } = engine();
    bridge.command("read_bootstrap", () => bootstrap);
    bridge.command("bootstrap_file", () => "/etc/packie/bootstrap.json");
    await expect(api.getEngineSettings()).resolves.toMatchObject({
      bootstrapPath: "/etc/packie/bootstrap.json",
    });
    bridge.command("bootstrap_file", () => null);
    const settings = await api.getEngineSettings();
    expect(settings.bootstrapPath ?? null).toBeNull();
  });

  it("fills what the bootstrap document leaves to its defaults", async () => {
    const { bridge, api } = engine();
    bridge.command("read_bootstrap", () => ({
      node: { node_id: "n", display_name: "N" },
      paths: { state_directory: "/var/lib/packie/" },
    }));
    await expect(api.getEngineSettings()).resolves.toMatchObject({
      stateDir: "/var/lib/packie/",
      logDir: "/var/lib/packie/logs",
      logRetentionDays: 30,
      logRingSize: 20_000,
    });
  });

  it("is mostly null without a bootstrap document", async () => {
    const { bridge, api } = engine();
    bridge.command("read_bootstrap", () => null);
    await expect(api.getEngineSettings()).resolves.toEqual({
      nodeId: "node-1",
      nodeName: "Lab 1",
      stateDir: null,
      logDir: null,
      ipcPath: null,
      hosting: "service",
      logRetentionDays: null,
      logRingSize: null,
    });
  });

  it("leaves the hosting null when the engine cannot be asked", async () => {
    const { bridge, api } = engine();
    bridge.command("read_bootstrap", () => null);
    bridge.request("get_snapshot", () => {
      throw { kind: "disconnected", message: "gone" };
    });
    await expect(api.getEngineSettings()).resolves.toMatchObject({ hosting: null });
  });

  it("lists the engine's secrets and the names the SFTP endpoints refer to", async () => {
    const { bridge, api } = engine();
    bridge.request("list_secrets", () => ({ type: "secrets", names: ["lab-key", "unused"] }));
    expect(await api.listSecrets()).toEqual([
      { name: "lab-key", storage: null, hasValue: true, usedByEndpointIds: [IDS.sftp] },
      { name: "lab-key-pass", storage: null, hasValue: false, usedByEndpointIds: [IDS.sftp] },
      { name: "unused", storage: null, hasValue: true, usedByEndpointIds: [] },
    ]);
  });

  it("stores and deletes secrets through the engine", async () => {
    const { bridge, api } = engine();
    bridge.request("set_secret", () => ({ type: "done" }));
    bridge.request("delete_secret", () => ({ type: "done" }));
    await api.setSecret("lab-key", "s3cret");
    await api.deleteSecret("lab-key");
    expect(bridge.sent("set_secret")).toEqual([
      { type: "set_secret", name: "lab-key", value: "s3cret" },
    ]);
    expect(bridge.sent("delete_secret")).toEqual([{ type: "delete_secret", name: "lab-key" }]);
  });

  it("rejects a secret the engine refuses", async () => {
    const { bridge, api } = engine();
    bridge.request("set_secret", () => {
      throw {
        kind: "ipc",
        error: {
          code: "bad_request",
          message: "invalid secret name",
          field_errors: [],
          reason_code: null,
        },
      };
    });
    await expect(api.setSecret("Bad Name", "x")).rejects.toMatchObject({
      message: "invalid secret name",
    });
  });

  it("describes the engine and the console", async () => {
    const { api } = engine();
    await expect(api.getEngineInfo()).resolves.toEqual({
      engineVersion: "0.1.0",
      consoleVersion: "0.1.0-console",
      protocolVersion: 3,
      buildCommit: null,
      platform: expect.any(String),
      license: "AGPL-3.0-only",
    });
  });

  it("rejects the engine info while no engine is connected", async () => {
    const { bridge, api } = engine();
    bridge.command("ipc_connection", () => ({ ...CONNECTED, state: "disconnected", engine: null }));
    await expect(api.getEngineInfo()).rejects.toMatchObject({ code: "engine_unavailable" });
  });
});

describe("files the shell saves", () => {
  it("saves a report the console built, naming only a suggested file name", async () => {
    const { bridge, api } = engine();
    bridge.command("save_report_file", () => ({ path: "/home/lab/report.csv" }));
    await expect(
      api.saveReportFile({ name: "report.csv", format: "csv", content: "a,b\r\n" }),
    ).resolves.toEqual({ savedTo: "/home/lab/report.csv" });
    expect(bridge.calls.find((call) => call.command === "save_report_file")?.args).toEqual({
      suggestedName: "report.csv",
      extension: "csv",
      content: "a,b\r\n",
    });
  });

  it("tells the engine about an export it does not see before anything is written", async () => {
    const { bridge, api } = engine();
    const order: string[] = [];
    bridge.request("record_client_export", () => {
      order.push("recorded");
      return { type: "done" };
    });
    bridge.command("save_report_file", () => {
      order.push("saved");
      return { path: "/home/lab/change.csv" };
    });
    await api.saveReportFile({
      name: "change.csv",
      format: "csv",
      content: "x",
      audit: { kind: "change_report", rows: 13 },
    });
    expect(order).toEqual(["recorded", "saved"]);
    expect(bridge.sent("record_client_export")).toEqual([
      { type: "record_client_export", kind: "change_report", rows: 13 },
    ]);
    // A report the engine already audits (a transmission CSV) is not recorded twice.
    await api.saveReportFile({ name: "t.csv", format: "csv", content: "x" });
    expect(bridge.sent("record_client_export")).toHaveLength(1);
  });

  it("writes nothing when the engine cannot record the export", async () => {
    const { bridge, api } = engine();
    bridge.request("record_client_export", () => {
      throw {
        kind: "ipc",
        error: {
          code: "unavailable",
          message: "The audit log cannot be written.",
          field_errors: [],
          reason_code: null,
        },
      };
    });
    bridge.command("save_report_file", () => ({ path: "/home/lab/change.csv" }));
    await expect(
      api.saveReportFile({
        name: "change.csv",
        format: "csv",
        content: "x",
        audit: { kind: "change_report", rows: 3 },
      }),
    ).rejects.toMatchObject({ code: "unavailable", message: "The audit log cannot be written." });
    expect(bridge.count("save_report_file")).toBe(0);
  });

  it("resolves with null when the operator cancels the save dialog", async () => {
    const { bridge, api } = engine();
    bridge.command("save_report_file", () => null);
    bridge.command("save_message_export", () => null);
    await expect(
      api.saveReportFile({ name: "r.json", format: "json", content: "{}" }),
    ).resolves.toBeNull();
    await expect(
      api.exportMessages({ filter: {}, format: "csv", includeBodies: false }),
    ).resolves.toBeNull();
  });

  it("exports the filter the grid shows, with bodies only when asked", async () => {
    const { bridge, api } = engine();
    bridge.command("save_message_export", () => ({ path: "/home/lab/m.jsonl", rows: 12 }));
    const result = await api.exportMessages({
      filter: { channelIds: ["hl7-results"], identifier: { value: " AS66 ", prefix: true } },
      format: "jsonl",
      includeBodies: true,
    });
    expect(result).toEqual({ savedTo: "/home/lab/m.jsonl", rows: 12 });
    const args = bridge.calls.find((call) => call.command === "save_message_export")?.args;
    expect(args).toMatchObject({
      format: "jsonl",
      include_bodies: true,
      filter: { channels: ["hl7-results"], identifier: { value: "AS66", prefix: true } },
    });
    expect(args?.suggestedName).toMatch(/^packie-messages-\d{4}-\d{2}-\d{2}\.jsonl$/);
  });

  it("words a failed save as the engine error the pages expect", async () => {
    const { bridge, api } = engine();
    bridge.command("save_report_file", () => {
      throw { kind: "internal", message: "cannot write /x/report.csv: permission denied" };
    });
    await expect(
      api.saveReportFile({ name: "report.csv", format: "csv", content: "x" }),
    ).rejects.toMatchObject({
      code: "internal",
      message: "cannot write /x/report.csv: permission denied",
    });
  });
});
