import type { TransmissionQuery } from "@/api/types";

/** The period of a report: a preset counts back from the moment it runs; custom uses From and To. */
export type PeriodRange = "24h" | "7d" | "30d" | "custom";

export const PERIOD_CHOICES: readonly { id: PeriodRange; label: string }[] = [
  { id: "24h", label: "Last 24 hours" },
  { id: "7d", label: "Last 7 days" },
  { id: "30d", label: "Last 30 days" },
  { id: "custom", label: "Custom period…" },
];

const PRESET_MS: Readonly<Record<Exclude<PeriodRange, "custom">, number>> = {
  "24h": 86_400_000,
  "7d": 7 * 86_400_000,
  "30d": 30 * 86_400_000,
};

/** The report form, as the URL holds it. Running the report is a separate, explicit action. */
export interface TransmissionView {
  channel?: string;
  range: PeriodRange;
  /** Custom period only: RFC 3339 times. */
  since?: string;
  until?: string;
  /** Narrows the rows shown and exported, not the totals. */
  destination?: string;
}

const text = (params: URLSearchParams, name: string) => params.get(name)?.trim() || undefined;

const validTime = (value: string | undefined) =>
  value !== undefined && !Number.isNaN(Date.parse(value)) ? value : undefined;

function isRange(value: string | null): value is PeriodRange {
  return PERIOD_CHOICES.some((choice) => choice.id === value);
}

/**
 * Reads the form from the URL; anything unknown is ignored, never an error. A `since` without
 * a `range` is a custom period, so a link can name its period with `since` and `until` alone.
 */
export function parseTransmissionView(params: URLSearchParams): TransmissionView {
  const since = validTime(text(params, "since"));
  const until = validTime(text(params, "until"));
  const given = params.get("range");
  const range: PeriodRange = isRange(given)
    ? given
    : since !== undefined || until !== undefined
      ? "custom"
      : "24h";
  const view: TransmissionView = { range };
  const channel = text(params, "channel");
  const destination = text(params, "destination");
  if (channel) view.channel = channel;
  if (range === "custom") {
    if (since) view.since = since;
    if (until) view.until = until;
  }
  if (destination) view.destination = destination;
  return view;
}

/** The URL parameters for the form: only what differs from the defaults. */
export function toTransmissionParams(view: TransmissionView): Record<string, string> {
  const params: Record<string, string> = {};
  if (view.channel) params.channel = view.channel;
  if (view.range !== "24h") params.range = view.range;
  if (view.range === "custom") {
    if (view.since) params.since = view.since;
    if (view.until) params.until = view.until;
  }
  if (view.destination) params.destination = view.destination;
  return params;
}

/** Why the period cannot be run yet, or null when it can. */
export function periodProblem(view: TransmissionView): string | null {
  if (view.range !== "custom") return null;
  if (view.since === undefined) return "Choose when the period starts.";
  if (view.until !== undefined && Date.parse(view.until) <= Date.parse(view.since)) {
    return "The period must end after it starts.";
  }
  return null;
}

/** The engine's query for the form, or null while a channel or a valid period is missing. */
export function toQuery(view: TransmissionView, now: number): TransmissionQuery | null {
  if (view.channel === undefined || periodProblem(view) !== null) return null;
  if (view.range === "custom") {
    const query: TransmissionQuery = { channelId: view.channel, since: view.since ?? "" };
    if (view.until !== undefined) query.until = view.until;
    return query;
  }
  return { channelId: view.channel, since: new Date(now - PRESET_MS[view.range]).toISOString() };
}
