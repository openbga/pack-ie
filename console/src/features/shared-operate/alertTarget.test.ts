import { describe, expect, it } from "vitest";
import type { AlertKind } from "@/api/types";
import { enginePageOf } from "./alertTarget";

describe("enginePageOf", () => {
  it.each([
    ["audit_unavailable", "/audit", "Audit log"],
    ["backup_stale", "/settings/backup", "Backup settings"],
    ["notifications_failing", "/settings/notifications", "Notification settings"],
  ] satisfies [AlertKind, string, string][])("sends %s to %s", (kind, to, label) => {
    expect(enginePageOf({ kind })).toEqual({ to, label });
  });

  it("has no page for an alert about a channel or an endpoint", () => {
    expect(enginePageOf({ kind: "endpoint_down" })).toBeNull();
    expect(enginePageOf({ kind: "disk_low" })).toBeNull();
  });
});
