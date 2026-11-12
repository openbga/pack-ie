import { describe, expect, it } from "vitest";
import type { ConfigDocument, MessageChannelConfig } from "../types";
import type { MockEngineApi } from "./index";
import { auSettings, mappingsFor } from "./mappingCatalog";
import { at, mockApiFactory, rejection } from "./mockTestSupport";

const create = mockApiFactory({ messageCount: 500, seed: 7 });

describe("configuration history", () => {
  it("keeps about 12 prior versions with historic documents", async () => {
    const api = create();
    const current = await api.getConfig();
    const versions = await api.listConfigVersions();
    expect(versions.length).toBeGreaterThanOrEqual(12);
    expect(versions[0]?.version).toBe(current.version);
    expect(versions[0]?.current).toBe(true);
    const appliers = new Set(versions.map((v) => v.appliedBy));
    expect(appliers).toContain("LAB\\jdoe");
    expect(appliers).toContain("LAB\\mlopez");
    // The newest versions have not been signed off yet; older ones have.
    expect(versions[0]?.verifications).toEqual([]);
    expect(versions.at(-1)?.verifications.length).toBeGreaterThan(0);
    const oldest = versions[versions.length - 1];
    const historic = await api.getConfigVersion(oldest?.version ?? 0);
    expect(historic.version).toBe(oldest?.version);
    expect(historic.channels.length).toBeLessThan(current.channels.length + 2);
    expect(JSON.stringify(historic)).not.toBe(JSON.stringify(current));
    await expect(api.getConfigVersion(9999)).rejects.toMatchObject({ code: "not_found" });
  });

  it("records what each historic apply did to every channel", async () => {
    const api = create();
    const versions = await api.listConfigVersions();
    const byNote = (note: string) => {
      const found = versions.find((v) => v.note === note);
      if (!found) throw new Error(`no version noted ${note}`);
      return found;
    };
    const changed = (note: string) =>
      byNote(note).outcomes.filter((o) => o.outcome !== "unchanged");
    // The startup import records none; every other version names every channel.
    expect(byNote("imported from the runtime file").outcomes).toEqual([]);
    expect(byNote("Dashboard feed keeps 30 days").outcomes).toHaveLength(36);
    expect(changed("Dashboard feed keeps 30 days")).toEqual([
      { channelId: "ch-msg-dashboard", outcome: "updated" },
    ]);
    expect(changed("Add the dashboard feed for the clinic display")).toEqual([
      { channelId: "ch-msg-dashboard", outcome: "started" },
    ]);
    expect(changed("Retire the Cobas legacy orders channel")).toEqual([
      { channelId: "ch-msg-cobas-legacy", outcome: "removed" },
    ]);
    const moved = changed("LIS moved to a new address after the server migration");
    expect(moved.length).toBeGreaterThan(5);
    expect(moved.every((o) => o.outcome === "restarted")).toBe(true);
    expect(changed("STA Compact baud rate set to 19200 after the cable swap")).toEqual([
      {
        channelId: "ch-msg-sta",
        outcome: "failed",
        reason: "Cannot open COM24: access denied",
      },
    ]);
  });

  it("previews a rollback with the engine's plan and changes nothing", async () => {
    const api = create();
    const before = await api.getConfig();
    // Version 46 differs from the active one only in the dashboard feed's retention.
    const preview = await api.previewRollback(before.version - 1);
    expect(preview.ok).toBe(true);
    expect(preview.plan?.channels.filter((c) => c.change !== "unchanged")).toEqual([
      { id: "ch-msg-dashboard", label: "Dashboard feed", change: "updated" },
    ]);
    await expect(api.previewRollback(before.version)).rejects.toMatchObject({ code: "conflict" });
    await expect(api.previewRollback(4242)).rejects.toMatchObject({ code: "not_found" });
    expect((await api.getConfig()).version).toBe(before.version);
  });

  it("rolls back as a new version with its source and restarts what differs", async () => {
    const api = create();
    const before = await api.getConfig();
    const target = before.version - 3;
    const result = await api.rollbackConfig(target, "the new mapping dropped a field");
    expect(result.version).toBe(before.version + 1);
    expect(result.restartedChannelIds.length + result.unchangedChannelCount).toBeGreaterThan(30);
    const latest = (await api.listConfigVersions())[0];
    expect(latest).toMatchObject({
      version: result.version,
      note: `rollback of ${target}: the new mapping dropped a field`,
    });
    const rolledBack = await api.getConfig();
    const old = await api.getConfigVersion(target);
    expect({ ...rolledBack, version: 0 }).toEqual({ ...old, version: 0 });
    await expect(api.rollbackConfig(result.version, "again")).rejects.toMatchObject({
      code: "conflict",
    });
    await expect(api.rollbackConfig(4242, "gone")).rejects.toMatchObject({ code: "not_found" });
  });

  it("returns documents consistent with the snapshot", async () => {
    const api = create();
    const doc: ConfigDocument = await api.getConfig();
    const snapshot = await api.getSnapshot();
    expect(snapshot.channels.map((c) => c.id).sort()).toEqual(doc.channels.map((c) => c.id).sort());
    expect(snapshot.endpoints.map((e) => e.id).sort()).toEqual(
      doc.endpoints.map((e) => e.id).sort(),
    );
    const kinds = new Set(doc.endpoints.map((e) => e.kind));
    expect(kinds.size).toBe(5);
    const serial = doc.endpoints.find((e) => e.kind === "serial");
    expect(serial?.kind === "serial" && serial.port).toMatch(/^COM\d+$/);
    const sftp = doc.endpoints.find((e) => e.kind === "sftp");
    expect(sftp?.kind === "sftp" && sftp.secretName).toBeTruthy();
  });
});

describe("an AU connector, created through the engine", () => {
  function auChannel(au: MessageChannelConfig["source"]["settings"]["au"]): MessageChannelConfig {
    const mapping = mappingsFor("au", "hl7_mllp");
    return {
      kind: "message",
      id: "au-new",
      label: "New AU line",
      enabled: true,
      source: {
        id: "src",
        endpointId: "ep-au-new",
        protocol: "au",
        settings: { au },
        duplicateDetection: true,
      },
      filters: [],
      destinations: [
        {
          id: "dst-1",
          label: "LIS",
          endpointId: "ep-lis-out",
          protocol: "hl7_mllp",
          settings: { ackMode: "enhanced" },
          enabled: true,
          filters: [],
          mappings: mapping.rules,
          codeTables: mapping.codeTables,
          retry: { initialMs: 2000, maxMs: 300_000, maxAttempts: 5 },
        },
      ],
      retention: { days: 30, maxMessages: null },
    };
  }

  async function withChannel(api: MockEngineApi, au: Parameters<typeof auChannel>[0]) {
    const doc = await api.getConfig();
    doc.endpoints.push({
      kind: "tcp_listener",
      id: "ep-au-new",
      label: "New AU680 link",
      enabled: true,
      bindAddress: "10.0.4.5",
      port: 5099,
      allowlist: ["10.0.4.60"],
      allowAnyPeer: false,
      idleTimeoutSeconds: 0,
      keepalive: true,
    });
    doc.channels.push(auChannel(au));
    return doc;
  }

  it("is refused while its required fields are empty, then applied once they are filled", async () => {
    const api = create();
    const empty = {
      profile: undefined,
      framingStart: [],
      framingEnd: [],
      bcc: false,
      useEtb: false,
      maxWireLen: 0,
      framingClass: undefined,
      unitNo: false,
      rackDigits: -1,
      sampleIdDigits: 0,
      sex: false,
      age: false,
      month: false,
      patientInfoDigits: [0, 0, 0, 0, 0, 0],
      resultDigits: 0,
      frameTimeoutMs: 0,
      blockTimeoutMs: 0,
    } as never;
    const doc = await withChannel(api, empty);
    const index = doc.channels.length - 1;
    const result = await api.validateConfig(doc);
    expect(result.ok).toBe(false);
    const base = `channels[${index}].source.settings.au`;
    expect(at(result.issues, "error").sort()).toEqual(
      [
        "profile",
        "framingStart",
        "framingEnd",
        "maxWireLen",
        "framingClass",
        "rackDigits",
        "sampleIdDigits",
        "resultDigits",
        "frameTimeoutMs",
        "blockTimeoutMs",
      ]
        .map((field) => `${base}.${field}`)
        .sort(),
    );
    const error = await rejection(api.applyConfig(doc, "empty AU"));
    expect(error.code).toBe("validation");
    expect(error.fieldErrors.map((f) => f.field)).toContain(`${base}.profile`);

    const fresh = create();
    const valid = await withChannel(fresh, auSettings());
    expect(at((await fresh.validateConfig(valid)).issues, "error")).toEqual([]);
    const applied = await fresh.applyConfig(valid, "AU line");
    expect(applied.version).toBe(valid.version + 1);
    const stored = (await fresh.getConfig()).channels.find((c) => c.id === "au-new");
    expect(stored?.kind === "message" && stored.source.settings.au).toEqual(auSettings());
  });
});
