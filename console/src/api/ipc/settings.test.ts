import { describe, expect, it } from "vitest";
import type { ConnectionPayload } from "./bridge";
import realBootstrap from "./real/bootstrap.json";
import { type BootstrapDocument, engineSettingsFrom, secretsFrom } from "./settings";
import { richConfig } from "./test/configFixtures";

const connection: ConnectionPayload = {
  state: "connected",
  endpoint: "/var/lib/packie/packie.sock",
  error: null,
  attempt: 0,
  engine: { engine_version: "0.1.0", protocol_version: 1, node_id: "n", node_name: "N" },
  since: "2026-11-06T12:00:00Z",
};

describe("engine settings from a bootstrap document the engine accepts", () => {
  // `bootstrap.json` as the engine's own configuration has it: node.*, paths.*, logging.*, and
  // sections the console does not show.
  const bootstrap = realBootstrap as unknown as BootstrapDocument;

  it("reads the node, the directories and the log settings", () => {
    expect(engineSettingsFrom(bootstrap, connection, "console")).toEqual({
      nodeId: "node-1",
      nodeName: "Test node",
      stateDir: "/var/lib/packie",
      logDir: "/var/log/packie",
      ipcPath: "/var/lib/packie/packie.sock",
      hosting: "console",
      logRetentionDays: 7,
      logRingSize: 20_000,
      notifications: null,
    });
  });

  it("takes the node from the connection when there is no document", () => {
    expect(engineSettingsFrom(null, connection)).toEqual({
      nodeId: "n",
      nodeName: "N",
      stateDir: null,
      logDir: null,
      ipcPath: null,
      hosting: null,
      logRetentionDays: null,
      logRingSize: null,
    });
  });

  it("falls back to the specification's defaults for what the document leaves out", () => {
    const minimal: BootstrapDocument = {
      node: { node_id: "n", display_name: "N" },
      paths: { state_directory: "/state/" },
    };
    expect(engineSettingsFrom(minimal, null)).toMatchObject({
      logDir: "/state/logs",
      logRetentionDays: 30,
      logRingSize: 20_000,
      ipcPath: null,
    });
  });
});

describe("secrets", () => {
  it("joins what the engine holds with what the endpoints refer to", () => {
    expect(secretsFrom(richConfig(), ["unused", "lab-key"])).toEqual([
      { name: "lab-key", storage: null, hasValue: true, usedByEndpointIds: ["ep-sftp"] },
      { name: "lab-key-pass", storage: null, hasValue: false, usedByEndpointIds: ["ep-sftp"] },
      { name: "unused", storage: null, hasValue: true, usedByEndpointIds: [] },
    ]);
  });

  it("is empty when nothing is stored and nothing refers to a secret", () => {
    expect(secretsFrom({ endpoints: [], channels: [] }, [])).toEqual([]);
  });

  it("does not show a literal that sits where a reference belongs", () => {
    const config = richConfig();
    const sftp = config.endpoints?.find((e) => e.kind === "sftp");
    if (sftp?.kind !== "sftp") throw new Error("fixture");
    sftp.settings.auth = {
      method: "password",
      // An engine reads this as `secret_inline` and keeps no value; the console shows no name.
      password: "hunter2" as never,
    };
    expect(secretsFrom(config, [])).toEqual([]);
  });
});
