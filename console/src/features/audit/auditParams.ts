import type { AuditAction, AuditFilter, AuditObjectType, AuditOutcome } from "@/api/types";
import { ACTION_LABELS, type Choice, OBJECT_TYPE_LABELS, OUTCOME_LABELS } from "./auditLabels";

/** All time shows every event; the others look back from now; custom uses From and To. */
export type AuditRange = "1h" | "24h" | "7d" | "30d" | "all" | "custom";

export const RANGE_CHOICES: readonly Choice<AuditRange>[] = [
  { id: "1h", label: "Last hour" },
  { id: "24h", label: "Last 24 hours" },
  { id: "7d", label: "Last 7 days" },
  { id: "30d", label: "Last 30 days" },
  { id: "all", label: "All time" },
  { id: "custom", label: "Custom range…" },
];

const RANGE_MS: Readonly<Record<Exclude<AuditRange, "all" | "custom">, number>> = {
  "1h": 3_600_000,
  "24h": 86_400_000,
  "7d": 7 * 86_400_000,
  "30d": 30 * 86_400_000,
};

/** The Audit filters, as the URL holds them. Unset fields do not narrow. */
export interface AuditView {
  range: AuditRange;
  /** Custom range only: RFC 3339 times. */
  from?: string;
  to?: string;
  actor?: string;
  action?: AuditAction;
  objectType?: AuditObjectType;
  objectId?: string;
  outcome?: AuditOutcome;
}

export const DEFAULT_VIEW: AuditView = { range: "all" };

const text = (params: URLSearchParams, name: string) => params.get(name)?.trim() || undefined;

function oneOf<Code extends string>(labels: Readonly<Record<Code, string>>, value: string | null) {
  return value !== null && Object.hasOwn(labels, value) ? (value as Code) : undefined;
}

function isRange(value: string | null): value is AuditRange {
  return RANGE_CHOICES.some((range) => range.id === value);
}

/** Reads the filters from the URL; anything unknown is ignored, never an error. */
export function parseAuditView(params: URLSearchParams): AuditView {
  const range = params.get("range");
  const view: AuditView = { range: isRange(range) ? range : "all" };
  if (view.range === "custom") {
    const from = text(params, "from");
    const to = text(params, "to");
    if (from) view.from = from;
    if (to) view.to = to;
  }
  const actor = text(params, "actor");
  const objectId = text(params, "id");
  const action = oneOf(ACTION_LABELS, params.get("action"));
  const objectType = oneOf(OBJECT_TYPE_LABELS, params.get("type"));
  const outcome = oneOf(OUTCOME_LABELS, params.get("outcome"));
  if (actor) view.actor = actor;
  if (action) view.action = action;
  if (objectType) view.objectType = objectType;
  if (objectId) view.objectId = objectId;
  if (outcome) view.outcome = outcome;
  return view;
}

/** The URL parameters for the filters: only what differs from the defaults. */
export function toAuditParams(view: AuditView): Record<string, string> {
  const params: Record<string, string> = {};
  if (view.range !== "all") params.range = view.range;
  if (view.range === "custom") {
    if (view.from) params.from = view.from;
    if (view.to) params.to = view.to;
  }
  if (view.actor) params.actor = view.actor;
  if (view.action) params.action = view.action;
  if (view.objectType) params.type = view.objectType;
  if (view.objectId) params.id = view.objectId;
  if (view.outcome) params.outcome = view.outcome;
  return params;
}

export function isFiltered(view: AuditView): boolean {
  return Object.keys(toAuditParams(view)).length > 0;
}

const validTime = (value: string | undefined) =>
  value !== undefined && !Number.isNaN(new Date(value).getTime()) ? value : undefined;

/** The time bounds of a view: a preset counts back from `now`. */
function timeBounds(view: AuditView, now: number): Pick<AuditFilter, "since" | "until"> {
  if (view.range === "all") return {};
  if (view.range === "custom") {
    const bounds: Pick<AuditFilter, "since" | "until"> = {};
    const since = validTime(view.from);
    const until = validTime(view.to);
    if (since) bounds.since = since;
    if (until) bounds.until = until;
    return bounds;
  }
  return { since: new Date(now - RANGE_MS[view.range]).toISOString() };
}

/** The engine's filter for a view. */
export function toAuditFilter(view: AuditView, now: number): AuditFilter {
  const filter: AuditFilter = { ...timeBounds(view, now) };
  if (view.actor) filter.actor = view.actor;
  if (view.action) filter.action = view.action;
  if (view.objectType) filter.objectType = view.objectType;
  if (view.objectId) filter.objectId = view.objectId;
  if (view.outcome) filter.outcome = view.outcome;
  return filter;
}

/** The filters in words, for the export dialog: "action Channel paused", "actor LAB\jdoe". */
export function describeView(view: AuditView): string[] {
  const parts: string[] = [];
  const range = RANGE_CHOICES.find((choice) => choice.id === view.range);
  if (view.range !== "all" && range) {
    parts.push(view.range === "custom" ? "custom time range" : range.label.toLowerCase());
  }
  if (view.actor) parts.push(`actor ${view.actor}`);
  if (view.action) parts.push(`action ${ACTION_LABELS[view.action]}`);
  if (view.objectType) parts.push(`object type ${OBJECT_TYPE_LABELS[view.objectType]}`);
  if (view.objectId) parts.push(`object ID ${view.objectId}`);
  if (view.outcome) parts.push(`outcome ${OUTCOME_LABELS[view.outcome]}`);
  return parts;
}
