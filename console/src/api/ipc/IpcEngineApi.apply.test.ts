import { describe, expect, it } from "vitest";
import { documentFromWire } from "./configDocument";
import { richConfig } from "./test/configFixtures";
import { actor, applied, edited, engine, problem } from "./test/ipcEngine";
import type { AnyRequest, WireConfig } from "./wire";

describe("configuration", () => {
  it("applies on the version it was edited from, with the note, and reports what restarted", async () => {
    const { bridge, api, config } = engine();
    bridge.request("put_config", () =>
      applied([
        { channel_id: "a", outcome: "restarted" },
        { channel_id: "b", outcome: "unchanged" },
        { channel_id: "c", outcome: "started" },
        { channel_id: "d", outcome: "unchanged" },
        { channel_id: "e", outcome: "updated" },
      ]),
    );
    const result = await api.applyConfig(await edited(api), "  raise the baud rate ");
    expect(result).toEqual({
      version: 8,
      outcomes: [
        { channelId: "a", outcome: "restarted" },
        { channelId: "b", outcome: "unchanged" },
        { channelId: "c", outcome: "started" },
        { channelId: "d", outcome: "unchanged" },
        { channelId: "e", outcome: "updated" },
      ],
      restartedChannelIds: ["a"],
      unchangedChannelCount: 2,
      failures: [],
    });
    const put = bridge.sent("put_config")[0];
    expect(put).toMatchObject({
      type: "put_config",
      base_version: 7,
      note: "raise the baud rate",
    });
    expect(put?.type === "put_config" && put.document.config_version).toBe(7);
    expect(put?.type === "put_config" && put.document.generated_at).toBe(config.generated_at);

    // The next read goes to the engine again.
    const before = bridge.sent("get_config").length;
    await api.getSnapshot();
    expect(bridge.sent("get_config").length).toBe(before + 1);
  });

  it("reports the channels that could not be applied with the engine's reason", async () => {
    const { bridge, api } = engine();
    bridge.request("put_config", () => ({
      type: "config_applied",
      config_version: 8,
      result: "partial",
      outcomes: [
        { channel_id: "a", outcome: "failed", reason: "port COM3 is in use" },
        { channel_id: "b", outcome: "restarted" },
        { channel_id: "c", outcome: "removed" },
      ],
      diff: { channels: [], endpoints: [], alerts_changed: false },
      warnings: [],
    }));
    await expect(api.applyConfig(await edited(api), "partial apply")).resolves.toEqual({
      version: 8,
      outcomes: [
        { channelId: "a", outcome: "failed", reason: "port COM3 is in use" },
        { channelId: "b", outcome: "restarted" },
        { channelId: "c", outcome: "removed" },
      ],
      restartedChannelIds: ["b"],
      unchangedChannelCount: 0,
      failures: [{ channelId: "a", reason: "port COM3 is in use" }],
    });
  });

  it("refuses to apply, verify or roll back without a note, sending nothing", async () => {
    const { bridge, api } = engine();
    bridge.request("put_config", () => applied());
    const document = await edited(api);
    for (const note of ["", "  ", "x".repeat(501)]) {
      await expect(api.applyConfig(document, note)).rejects.toMatchObject({
        code: "validation",
        fieldErrors: [{ field: "note" }],
      });
    }
    await expect(api.verifyConfigVersion(7, " ")).rejects.toMatchObject({ code: "validation" });
    await expect(api.rollbackConfig(5, "")).rejects.toMatchObject({ code: "validation" });
    expect(bridge.sent("put_config")).toEqual([]);
    expect(bridge.sent("verify_config_version")).toEqual([]);
  });

  it("does not send a document the console cannot encode", async () => {
    const { bridge, api } = engine();
    const doc = await api.getConfig();
    const channel = doc.channels[1];
    const csv = channel?.kind === "message" ? channel.destinations[1] : undefined;
    if (csv) csv.settings.templateJson = "nope";
    await expect(api.applyConfig(doc, "bad template")).rejects.toMatchObject({
      code: "validation",
      fieldErrors: [{ field: "channels[1].destinations[1].settings.templateJson" }],
    });
    expect(bridge.sent("put_config")).toEqual([]);
  });

  it("surfaces the engine's validation and conflict answers", async () => {
    const { bridge, api } = engine();
    bridge.request("put_config", () => {
      throw {
        kind: "ipc",
        error: {
          code: "invalid_config",
          message: "1 configuration error",
          field_errors: [
            problem("channels[1].retention.max_age_days", "must be greater than 0"),
            problem("channels[1].retention", "keeps everything", "warning"),
          ],
          reason_code: "invalid_config",
        },
      };
    });
    await expect(api.applyConfig(await api.getConfig(), "bad retention")).rejects.toMatchObject({
      code: "validation",
      fieldErrors: [{ field: "channels[1].retention.days", message: "must be greater than 0" }],
    });
    bridge.request("put_config", () => {
      throw {
        kind: "ipc",
        error: {
          code: "conflict",
          message: "stale",
          field_errors: [],
          reason_code: "version_conflict",
        },
      };
    });
    await expect(api.applyConfig(await api.getConfig(), "stale")).rejects.toMatchObject({
      code: "conflict",
    });
  });

  it("lists versions with the current one marked, and the note of each", async () => {
    const { bridge, api } = engine();
    bridge.request("get_config_history", () => ({
      type: "config_history",
      versions: [
        {
          config_version: 7,
          generated_at: "2026-11-06T12:00:00Z",
          endpoints: 6,
          channels: 2,
          note: "raise the baud rate",
          applied_by: actor("LAB\\jdoe"),
          applied_at: "2026-11-06T12:00:05Z",
          outcomes: [],
          verifications: [
            {
              verified_by: actor("LAB\\mlopez"),
              verified_at: "2026-11-06T15:00:00Z",
              note: "VR-2026-018",
            },
          ],
        },
        {
          config_version: 6,
          generated_at: "2026-11-05T12:00:00Z",
          endpoints: 1,
          channels: 1,
          note: null,
          applied_by: null,
          applied_at: null,
          outcomes: [],
          verifications: [],
        },
      ],
    }));
    expect(await api.listConfigVersions()).toEqual([
      {
        version: 7,
        appliedAt: "2026-11-06T12:00:05Z",
        appliedBy: "LAB\\jdoe",
        note: "raise the baud rate",
        endpoints: 6,
        channels: 2,
        outcomes: [],
        verifications: [{ by: "LAB\\mlopez", at: "2026-11-06T15:00:00Z", note: "VR-2026-018" }],
        current: true,
      },
      {
        // A version saved before the record was kept: no applier, the generation time stands in.
        version: 6,
        appliedAt: "2026-11-05T12:00:00Z",
        appliedBy: null,
        note: null,
        endpoints: 1,
        channels: 1,
        outcomes: [],
        verifications: [],
        current: false,
      },
    ]);
  });

  it("signs a version off with the engine's reply mapped like the history", async () => {
    const { bridge, api } = engine();
    bridge.request("verify_config_version", () => ({
      type: "config_version_verified",
      version: {
        config_version: 6,
        generated_at: "2026-11-05T12:00:00Z",
        endpoints: 1,
        channels: 1,
        note: null,
        applied_by: actor("LAB\\jdoe"),
        applied_at: "2026-11-05T12:00:03Z",
        outcomes: [],
        verifications: [
          { verified_by: actor("LAB\\mlopez"), verified_at: "2026-11-06T09:00:00Z", note: "VR-7" },
        ],
      },
    }));
    const version = await api.verifyConfigVersion(6, " VR-7 ");
    expect(bridge.sent("verify_config_version")).toEqual([
      { type: "verify_config_version", config_version: 6, note: "VR-7" },
    ]);
    expect(version).toMatchObject({
      version: 6,
      outcomes: [],
      verifications: [{ by: "LAB\\mlopez", note: "VR-7" }],
      current: false,
    });
  });

  it("rolls back by applying an older document against the active version", async () => {
    const { bridge, api, config } = engine();
    const older: WireConfig = {
      ...richConfig(),
      config_version: 5,
      alerts: { ...config.alerts, disk_low_megabytes: 1 },
    };
    bridge.request("get_config_version", (request) => ({
      type: "config",
      document:
        request.type === "get_config_version" && request.config_version === 5 ? older : config,
    }));
    bridge.request("put_config", () => applied());
    await expect(api.rollbackConfig(5, "the new mapping dropped a field")).resolves.toEqual({
      version: 8,
      outcomes: [],
      restartedChannelIds: [],
      unchangedChannelCount: 0,
      failures: [],
    });
    const put = bridge.sent("put_config")[0] as Extract<AnyRequest, { type: "put_config" }>;
    expect(put.document.config_version).toBe(7);
    expect(put.base_version).toBe(7);
    expect(put.note).toBe("rollback of 5: the new mapping dropped a field");
    expect(put.document.alerts?.disk_low_megabytes).toBe(1);
    expect(bridge.sent("get_config_version")).toEqual([
      { type: "get_config_version", config_version: 5 },
    ]);
  });

  it("previews a rollback with the engine's plan, validating the stored document over the active version", async () => {
    const { bridge, api, config } = engine();
    const older: WireConfig = { ...richConfig(), config_version: 5 };
    bridge.request("get_config_version", (request) => ({
      type: "config",
      document:
        request.type === "get_config_version" && request.config_version === 5 ? older : config,
    }));
    bridge.request("validate_config", () => ({
      type: "validation",
      problems: [],
      diff: {
        channels: [
          { channel_id: "c1", label: "Dashboard feed", change: "updated" as const },
          { channel_id: "c2", label: "Billing", change: "unchanged" as const },
        ],
        endpoints: [],
        alerts_changed: false,
      },
    }));
    const preview = await api.previewRollback(5);
    expect(preview).toMatchObject({
      ok: true,
      plan: {
        channels: [
          { id: "c1", label: "Dashboard feed", change: "updated" },
          { id: "c2", label: "Billing", change: "unchanged" },
        ],
      },
    });
    const sent = bridge.sent("validate_config")[0];
    expect(sent?.type === "validate_config" && sent.document.config_version).toBe(7);
    expect(bridge.sent("put_config")).toEqual([]);
  });

  it("reads a document the same way as the mapping does", async () => {
    const { api, config } = engine();
    expect(await api.getConfig()).toEqual(documentFromWire(config));
  });
});
