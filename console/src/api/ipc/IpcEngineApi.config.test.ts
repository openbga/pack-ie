import { describe, expect, it } from "vitest";
import { edited, engine, problem, validation } from "./test/ipcEngine";

describe("configuration", () => {
  it("reads the active document and a stored version", async () => {
    const { api } = engine();
    expect((await api.getConfig()).version).toBe(7);
    expect((await api.getConfigVersion(7)).endpoints).toHaveLength(7);
    await expect(api.getConfigVersion(3)).rejects.toMatchObject({ code: "not_found" });
  });

  it("validates the edited document, keeping what the console does not model", async () => {
    const { bridge, api } = engine();
    bridge.request("validate_config", () =>
      validation([
        problem("endpoints[0].settings.port_name", "must not be empty"),
        problem("channels[0].connection.receiver", "required for receiver_then_sender"),
      ]),
    );
    const result = await api.validateConfig(await edited(api));
    expect(result).toMatchObject({
      ok: false,
      issues: [
        {
          severity: "error",
          code: "schema",
          path: "endpoints[0].port",
          message: "must not be empty",
        },
        {
          severity: "error",
          code: "schema",
          path: "channels[0].pairingReceiver",
          message: "required for receiver_then_sender",
        },
      ],
    });
    const sent = bridge.sent("validate_config")[0];
    const document = sent?.type === "validate_config" ? sent.document : null;
    expect(document?.endpoints?.[0]).toMatchObject({
      kind: "serial",
      tags: { site: "north" },
      settings: { baud_rate: 38_400, read_timeout_ms: 500, write_timeout_ms: 4000 },
    });
    expect(document?.config_version).toBe(7);
    expect(document?.schema_version).toBe(1);
    expect(document?.diagnostics).toEqual({ snapshot_interval_seconds: 5 });
  });

  it("carries the engine's plan for a valid document, and none for an invalid one", async () => {
    const { bridge, api } = engine();
    const diff = {
      channels: [
        { channel_id: "c1", label: "Chemistry", change: "changed" as const },
        { channel_id: "c2", label: "Billing", change: "unchanged" as const },
      ],
      endpoints: [{ endpoint_id: "e1", label: "Analyzer", change: "added" as const }],
      alerts_changed: true,
    };
    bridge.request("validate_config", () => ({ type: "validation", problems: [], diff }));
    const valid = await api.validateConfig(await api.getConfig());
    expect(valid.plan).toEqual({
      channels: [
        { id: "c1", label: "Chemistry", change: "changed" },
        { id: "c2", label: "Billing", change: "unchanged" },
      ],
      endpoints: [{ id: "e1", label: "Analyzer", change: "added" }],
      alertsChanged: true,
    });
    bridge.request("validate_config", () => validation([problem("channels[0].id", "taken")]));
    // The engine's plan of a document with errors is empty.
    expect((await api.validateConfig(await api.getConfig())).plan).toEqual({
      channels: [],
      endpoints: [],
      alertsChanged: false,
    });
  });

  it("keeps the severity of what the engine reports; warnings do not fail the check", async () => {
    const { bridge, api } = engine();
    bridge.request("validate_config", () =>
      validation([
        problem("channels[1].retention", "keeps everything", "warning"),
        problem("channels[1].destinations[0].mapping.rules[0].to", "unknown field"),
      ]),
    );
    const result = await api.validateConfig(await api.getConfig());
    expect(result.ok).toBe(false);
    expect(result.issues).toEqual([
      {
        severity: "warning",
        code: "schema",
        path: "channels[1].retention",
        message: "keeps everything",
      },
      {
        severity: "error",
        code: "schema",
        path: "channels[1].destinations[0].mappings[0].to",
        message: "unknown field",
      },
    ]);
    bridge.request("validate_config", () =>
      validation([problem("channels[1].retention", "keeps everything", "warning")]),
    );
    const warned = await api.validateConfig(await api.getConfig());
    expect(warned.ok).toBe(true);
    expect(warned.issues).toHaveLength(1);
  });

  it("reports ok when the engine finds nothing, and adds the console's own findings", async () => {
    const { bridge, api } = engine();
    bridge.request("validate_config", () => validation());
    const doc = await api.getConfig();
    await expect(api.validateConfig(doc)).resolves.toMatchObject({ ok: true, issues: [] });

    const channel = doc.channels[1];
    const csv = channel?.kind === "message" ? channel.destinations[1] : undefined;
    if (csv) csv.settings.templateJson = "{";
    const result = await api.validateConfig(doc);
    expect(result.ok).toBe(false);
    expect(result.issues).toEqual([
      {
        severity: "error",
        path: "channels[1].destinations[1].settings.templateJson",
        message: "The template is not valid JSON.",
      },
    ]);
  });
});
