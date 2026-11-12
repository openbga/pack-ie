// The engine behind `IpcEngineApi`'s tests: a `FakeBridge` that serves a configuration and a
// snapshot of it, and the engine's answers the configuration tests build on.

import type { ChannelOutcome, Problem } from "@/ipc/generated";
import type { ConfigDocument } from "../../types";
import type { ConnectionPayload } from "../bridge";
import { IpcEngineApi } from "../IpcEngineApi";
import type { AnyResponse, WireConfig } from "../wire";
import { richConfig } from "./configFixtures";
import { FakeBridge } from "./fakeBridge";
import { engineSnapshot } from "./snapshotFixtures";

const ENGINE = {
  engine_version: "0.1.0",
  protocol_version: 3,
  node_id: "node-1",
  node_name: "Lab 1",
  actor: { name: "LAB\\jdoe" },
};
export const CONNECTED: ConnectionPayload = {
  state: "connected",
  endpoint: "/run/packie.sock",
  error: null,
  attempt: 0,
  engine: ENGINE,
  since: "2026-11-06T12:00:00Z",
};

/** An engine that serves `config` (mutable) and a snapshot of it. */
export function engine(config: WireConfig = richConfig()) {
  const bridge = new FakeBridge()
    .command("ipc_connection", () => CONNECTED)
    .command("ipc_subscribe", () => 1)
    .command("ipc_unsubscribe", () => null)
    .command("app_version", () => "0.1.0-console")
    .request("get_snapshot", () => ({
      type: "snapshot",
      snapshot: engineSnapshot(config.config_version ?? 0),
    }))
    .request("get_config", () => ({ type: "config", document: config }))
    .request("get_config_version", (request) => {
      if (request.type !== "get_config_version") throw new Error("unexpected");
      if (request.config_version !== config.config_version) {
        throw {
          kind: "ipc",
          error: {
            code: "not_found",
            message: "no such version",
            field_errors: [],
            reason_code: null,
          },
        };
      }
      return { type: "config", document: config };
    });
  return { bridge, api: new IpcEngineApi(bridge), config };
}

export const actor = (name: string) => ({
  name,
  uid_or_sid: "1001",
  origin: "console" as const,
  pid: null,
});

export const problem = (
  path: string,
  message: string,
  severity: "error" | "warning" = "error",
): Problem => ({ severity, code: "schema", path, message });

export const validation = (problems: Problem[] = []): AnyResponse => ({
  type: "validation",
  problems,
  diff: { channels: [], endpoints: [], alerts_changed: false },
});

export const applied = (outcomes: ChannelOutcome[] = [], config_version = 8): AnyResponse => ({
  type: "config_applied",
  config_version,
  result: "applied",
  outcomes,
  diff: { channels: [], endpoints: [], alerts_changed: false },
  warnings: [],
});

/** The active document with the first serial endpoint's baud rate changed. */
export async function edited(api: IpcEngineApi): Promise<ConfigDocument> {
  const doc = await api.getConfig();
  const serial = doc.endpoints[0];
  if (serial?.kind === "serial") serial.baudRate = 38_400;
  return doc;
}
