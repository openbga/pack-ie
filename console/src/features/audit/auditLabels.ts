import type {
  AuditAction,
  AuditEvent,
  AuditObjectType,
  AuditOrigin,
  AuditOutcome,
  AuditProblemKind,
} from "@/api/types";
import type { Tone } from "@/components/tones";

/** How each action reads in the grid and the filter; the raw code stays in the detail. */
export const ACTION_LABELS: Readonly<Record<AuditAction, string>> = {
  config_validate: "Configuration validated",
  config_apply: "Configuration applied",
  config_rollback: "Configuration rolled back",
  config_verify: "Configuration version verified",
  channel_pause: "Channel paused",
  channel_resume: "Channel resumed",
  channel_restart: "Channel restarted",
  message_resend: "Message resent",
  delivery_close: "Delivery closed",
  secret_set: "Secret stored",
  secret_delete: "Secret removed",
  message_body_view: "Message body viewed",
  message_fields_view: "Message fields viewed",
  message_search: "Messages searched",
  message_export: "Messages exported",
  transmission_verify: "Transmission verified",
  audit_export: "Audit log exported",
  audit_verify: "Audit chain verified",
  backup_create: "Backup created",
  backup_restore: "Backup restored",
  traffic_view: "Traffic transcript opened",
  client_export: "Client export written",
  endpoint_test: "Endpoint tested",
  notification_test: "Notification test sent",
  engine_start: "Engine started",
  engine_stop: "Engine stopped",
  access_refused: "Access refused",
  audit_anchor: "Audit anchor published",
  audit_seal: "Audit file sealed",
  audit_prune: "Audit file removed by retention",
};

export const OBJECT_TYPE_LABELS: Readonly<Record<AuditObjectType, string>> = {
  channel: "Channel",
  message: "Message",
  delivery: "Delivery",
  config_version: "Configuration version",
  secret: "Secret",
  backup: "Backup",
  audit: "Audit log",
  engine: "Engine",
};

export const OUTCOME_LABELS: Readonly<Record<AuditOutcome, string>> = {
  requested: "Requested",
  succeeded: "Succeeded",
  rejected: "Rejected",
  failed: "Failed",
};

export const ORIGIN_LABELS: Readonly<Record<AuditOrigin, string>> = {
  console: "Console",
  cli: "Command line",
  engine: "Engine",
  other: "Other",
};

/** A command that was refused changed nothing (warning); one that broke is a failure (danger). */
export const OUTCOME_TONES: Readonly<Record<AuditOutcome, Tone>> = {
  succeeded: "neutral",
  requested: "info",
  rejected: "warning",
  failed: "danger",
};

export const PROBLEM_LABELS: Readonly<Record<AuditProblemKind, string>> = {
  hash_mismatch: "An event was edited",
  broken_link: "Events were removed, inserted or reordered",
  sequence_gap: "Events are missing",
  unexplained_start: "The oldest events are missing without a record",
  unreadable: "A file could not be read",
};

export interface Choice<Code extends string> {
  id: Code;
  label: string;
}

function choices<Code extends string>(labels: Readonly<Record<Code, string>>): Choice<Code>[] {
  return (Object.keys(labels) as Code[]).map((id) => ({ id, label: labels[id] }));
}

export const ACTION_CHOICES = choices(ACTION_LABELS);
export const OBJECT_TYPE_CHOICES = choices(OBJECT_TYPE_LABELS);
export const OUTCOME_CHOICES = choices(OUTCOME_LABELS);

/** Detail keys whose plain humanizing reads as an instruction rather than what was done. */
const DETAIL_LABELS: Readonly<Record<string, string>> = {
  confirm_superseded: "Superseded message confirmed",
  confirm_duplicate_risk: "Duplicate risk confirmed",
};

/** `base_version` as "Base version"; a few keys have a label of their own. */
export function humanizeKey(key: string): string {
  const label = DETAIL_LABELS[key];
  if (label !== undefined) return label;
  const words = key.replaceAll(/[_-]+/g, " ").trim();
  return words === "" ? key : words.charAt(0).toUpperCase() + words.slice(1);
}

/** A details value as text: strings whole, everything else as compact JSON. */
export function detailText(value: unknown): string {
  if (typeof value === "string") return value;
  return JSON.stringify(value) ?? String(value);
}

/** The console page of an event's object, where there is one. */
export function objectHref(event: Pick<AuditEvent, "objectType" | "objectId">): string | null {
  const { objectType, objectId } = event;
  if (objectId === null || objectId === "") return null;
  const id = encodeURIComponent(objectId);
  switch (objectType) {
    case "channel":
      return `/channels/${id}`;
    case "message":
      return `/messages/${id}`;
    case "config_version":
      return `/settings/versions?version=${id}`;
    default:
      return null;
  }
}
