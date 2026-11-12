import type { Alert } from "@/api/types";

/**
 * Where to deal with an alert that concerns no channel or endpoint but the engine's own
 * housekeeping: the audit log, backups and notifications have a page each.
 */
export function enginePageOf(alert: Pick<Alert, "kind">): { to: string; label: string } | null {
  switch (alert.kind) {
    case "audit_unavailable":
      return { to: "/audit", label: "Audit log" };
    case "backup_stale":
      return { to: "/settings/backup", label: "Backup settings" };
    case "notifications_failing":
      return { to: "/settings/notifications", label: "Notification settings" };
    default:
      return null;
  }
}
