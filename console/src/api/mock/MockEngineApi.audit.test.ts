import { describe, expect, it } from "vitest";
import type { AuditFilter } from "../types";
import type { MockEngineApi } from "./index";
import { mockApiFactory, query, rejection } from "./mockTestSupport";

const create = mockApiFactory({ messageCount: 500, seed: 7 });
const all = { filter: {} as AuditFilter, beforeSeq: null, limit: 1000 };

async function runningChannel(api: MockEngineApi): Promise<string> {
  const found = (await api.getSnapshot()).channels.find((each) => each.state === "running");
  if (found === undefined) throw new Error("the mock has no running channel");
  return found.id;
}

describe("the audit log", () => {
  it("lists events newest first, a page at a time, and filters exactly", async () => {
    const api = create();
    const first = await api.getAuditEvents({ ...all, limit: 50 });
    expect(first.events).toHaveLength(50);
    const seqs = first.events.map((event) => event.seq);
    expect(seqs).toEqual([...seqs].sort((a, b) => b - a));
    expect(first.nextBeforeSeq).toBe(seqs.at(-1));
    const second = await api.getAuditEvents({ ...all, limit: 50, beforeSeq: first.nextBeforeSeq });
    expect(second.events[0]?.seq).toBe((seqs.at(-1) ?? 0) - 1);

    const applies = await api.getAuditEvents({ ...all, filter: { action: "config_apply" } });
    expect(applies.events.length).toBeGreaterThan(0);
    expect(applies.events.every((event) => event.action === "config_apply")).toBe(true);
    expect(applies.events.some((event) => typeof event.details.note === "string")).toBe(true);
    const someone = applies.events[0]?.actor ?? "";
    const theirs = await api.getAuditEvents({ ...all, filter: { actor: someone } });
    expect(theirs.events.every((event) => event.actor === someone)).toBe(true);
    const none = await api.getAuditEvents({ ...all, filter: { actor: someone.slice(0, 3) } });
    expect(none.events).toEqual([]);
  });

  it("records what the console does: commands twice, content views and searches once", async () => {
    const api = create();
    const before = (await api.getAuditEvents(all)).events[0]?.seq ?? 0;
    const channelId = await runningChannel(api);
    await api.pauseChannel(channelId);
    const row = (await api.listMessages(query({}, { limit: 1 }))).rows[0];
    await api.getMessageBody({ messageId: row?.id ?? "", which: { kind: "original" }, as: "text" });
    await api.getMessageFields(row?.id ?? "");
    await api.listMessages(query({ text: "ORU" }));
    const added = (await api.getAuditEvents({ ...all, limit: 20 })).events
      .filter((event) => event.seq > before)
      .reverse();
    expect(added.map((event) => [event.action, event.outcome])).toEqual([
      ["channel_pause", "requested"],
      ["channel_pause", "succeeded"],
      ["message_body_view", "succeeded"],
      ["message_fields_view", "succeeded"],
      ["message_search", "succeeded"],
    ]);
    expect(added[0]).toMatchObject({
      objectType: "channel",
      objectId: channelId,
      origin: "console",
    });
    expect(added[4]?.details).toMatchObject({ text: "ORU" });
  });

  it("files a refused command as rejected, and reading the log is not recorded", async () => {
    const api = create();
    await rejection(api.resumeChannel(await runningChannel(api)));
    const events = (await api.getAuditEvents({ ...all, limit: 2 })).events;
    expect(events.map((event) => [event.action, event.outcome])).toEqual([
      ["channel_resume", "rejected"],
      ["channel_resume", "requested"],
    ]);
    const again = await api.getAuditEvents({ ...all, limit: 1 });
    expect(again.events[0]?.seq).toBe(events[0]?.seq);
  });

  it("fails closed: nothing audited runs while the log cannot be written, and an alert opens", async () => {
    const api = create();
    const channelId = await runningChannel(api);
    api.setAuditAvailable(false);
    expect(await rejection(api.pauseChannel(channelId))).toMatchObject({ code: "unavailable" });
    const row = (await api.listMessages(query({}, { limit: 1 }))).rows[0];
    expect(
      await rejection(
        api.getMessageBody({ messageId: row?.id ?? "", which: { kind: "original" }, as: "text" }),
      ),
    ).toMatchObject({ code: "unavailable" });
    expect(await rejection(api.getMessageFields(row?.id ?? ""))).toMatchObject({
      code: "unavailable",
    });
    const alert = (await api.listAlerts("open")).find((each) => each.kind === "audit_unavailable");
    expect(alert).toMatchObject({ severity: "critical", title: "The audit log cannot be written" });
    expect(alert?.nextAction).toContain("<state>/audit");
    // The channel was not paused.
    const channel = (await api.getSnapshot()).channels.find((each) => each.id === channelId);
    expect(channel?.state).toBe("running");
    api.setAuditAvailable(true);
    await api.pauseChannel(channelId);
    expect((await api.listAlerts("open")).some((each) => each.kind === "audit_unavailable")).toBe(
      false,
    );
  });

  it("verifies the chain: intact, with a start explained by retention, and broken", async () => {
    const api = create();
    const intact = await api.verifyAudit();
    expect(intact).toMatchObject({ ok: true, firstSeq: 1, problems: [] });
    expect(intact.headHash).toMatch(/^[0-9a-f]{64}$/);
    api.pruneAudit(50);
    const pruned = await api.verifyAudit();
    expect(pruned.ok).toBe(true);
    expect(pruned.firstSeq).toBe(51);
    api.breakAuditChain(70);
    const broken = await api.verifyAudit();
    expect(broken.ok).toBe(false);
    expect(broken.problems).toEqual([expect.objectContaining({ seq: 70, kind: "hash_mismatch" })]);
    // Verifying is itself recorded.
    const latest = (await api.getAuditEvents({ ...all, filter: { action: "audit_verify" } }))
      .events;
    expect(latest.length).toBeGreaterThanOrEqual(3);
  });

  it("exports the filtered events oldest first with a CSV header, and records the export", async () => {
    const api = create();
    const result = await api.exportAudit({ format: "csv", filter: { action: "config_verify" } });
    const counted = (await api.getAuditEvents({ ...all, filter: { action: "config_verify" } }))
      .events;
    expect(result).toMatchObject({ rows: counted.length });
    const exported = (await api.getAuditEvents({ ...all, filter: { action: "audit_export" } }))
      .events;
    expect(exported[0]).toMatchObject({ outcome: "succeeded", details: { format: "csv" } });
  });
});

describe("change control", () => {
  it("refuses an apply, a rollback or a sign-off without a note, changing nothing", async () => {
    const api = create();
    const document = await api.getConfig();
    for (const note of ["", "   ", "x".repeat(501)]) {
      expect(await rejection(api.applyConfig(document, note))).toMatchObject({
        code: "validation",
      });
    }
    expect(await rejection(api.verifyConfigVersion(document.version, ""))).toMatchObject({
      code: "validation",
    });
    expect((await api.getConfig()).version).toBe(document.version);
    const rejected = (await api.getAuditEvents({ ...all, filter: { action: "config_apply" } }))
      .events;
    expect(rejected[0]).toMatchObject({ outcome: "rejected" });
  });

  it("signs a version off with the account and the reference, more than once", async () => {
    const api = create();
    const current = (await api.listConfigVersions())[0];
    expect(current?.verifications).toEqual([]);
    const once = await api.verifyConfigVersion(current?.version ?? 0, "VR-1");
    expect(once.verifications).toMatchObject([{ by: "LAB\\jdoe", note: "VR-1" }]);
    const twice = await api.verifyConfigVersion(current?.version ?? 0, "VR-2");
    expect(twice.verifications.map((each) => each.note)).toEqual(["VR-1", "VR-2"]);
    expect((await api.listConfigVersions())[0]?.verifications).toHaveLength(2);
    expect(await rejection(api.verifyConfigVersion(9999, "VR-3"))).toMatchObject({
      code: "not_found",
    });
  });
});

describe("backups", () => {
  it("reports the schedule and last success, goes stale, and a backup clears the alert", async () => {
    const api = create();
    const status = await api.getBackupStatus();
    expect(status).toMatchObject({
      stale: false,
      staleAfterHours: 26,
      schedule: { dailyAt: "02:00", keep: 14 },
    });
    expect(status.lastSuccess?.summary?.messages).toBe(500);
    api.makeBackupStale(40);
    expect((await api.getBackupStatus()).stale).toBe(true);
    const alert = (await api.listAlerts("open")).find((each) => each.kind === "backup_stale");
    expect(alert).toMatchObject({ severity: "warning" });
    expect(alert?.nextAction).toContain("packie backup create");
    const summary = await api.createBackup();
    expect(summary).toMatchObject({ messages: 500, files: expect.any(Number) });
    expect((await api.getBackupStatus()).stale).toBe(false);
    expect((await api.listAlerts("open")).some((each) => each.kind === "backup_stale")).toBe(false);
    const events = (await api.getAuditEvents({ ...all, filter: { action: "backup_create" } }))
      .events;
    expect(events.map((event) => event.outcome).slice(0, 2)).toEqual(["succeeded", "requested"]);
  });

  it("records a failed backup in the status, and refuses one while the audit log is down", async () => {
    const api = create();
    api.failNextBackup("No space left on device");
    expect(await rejection(api.createBackup())).toMatchObject({ code: "internal" });
    const status = await api.getBackupStatus();
    expect(status.lastAttempt).toMatchObject({ ok: false, error: "No space left on device" });
    expect(status.lastSuccess?.ok).toBe(true);
    api.setAuditAvailable(false);
    expect(await rejection(api.createBackup())).toMatchObject({ code: "unavailable" });
  });
});

describe("an export the console builds itself", () => {
  const report = {
    name: "change.csv",
    format: "csv" as const,
    content: "x",
    audit: { kind: "change_report" as const, rows: 13 },
  };

  it("is recorded before it is written: requested, then succeeded, with its kind and size", async () => {
    const api = create();
    const before = (await api.getAuditEvents(all)).events[0]?.seq ?? 0;
    await api.saveReportFile(report);
    const events = (await api.getAuditEvents({ ...all, filter: { action: "client_export" } }))
      .events;
    expect(events.map((event) => [event.outcome, event.seq > before])).toEqual([
      ["succeeded", true],
      ["requested", true],
    ]);
    expect(events[1]?.details).toEqual({ kind: "change_report", rows: 13 });
  });

  it("is not recorded again when the engine already audits it, and not at all without a request", async () => {
    const api = create();
    await api.saveReportFile({ name: "t.csv", format: "csv", content: "x" });
    const events = await api.getAuditEvents({ ...all, filter: { action: "client_export" } });
    expect(events.events).toEqual([]);
  });

  it("is refused while the audit log cannot be written", async () => {
    const api = create();
    api.setAuditAvailable(false);
    await expect(api.saveReportFile(report)).rejects.toMatchObject({ code: "unavailable" });
  });
});
