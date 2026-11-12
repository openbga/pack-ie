// Setup checks (`list_serial_ports`, `test_endpoint`, `test_notification`): the engine's
// reports as the console's, and a console endpoint as the engine's `EndpointTest`. Pure.

import type {
  NotificationResult as RawNotification,
  SerialPort as RawPort,
  ProbeReport as RawProbe,
} from "@/ipc/generated";
import type {
  EngineError,
  NotificationResult,
  NotificationSettings,
  ProbeReport,
  SerialPortInfo,
} from "../types";
import { toUiPath } from "./paths";
import type { WireConfig } from "./wire";

export function serialPortFromIpc(raw: RawPort): SerialPortInfo {
  return {
    name: raw.name,
    kind: raw.kind,
    vid: raw.vid,
    pid: raw.pid,
    serialNumber: raw.serial_number,
    manufacturer: raw.manufacturer,
    product: raw.product,
    usedBy: raw.used_by.map((use) => ({
      endpointId: use.endpoint_id,
      endpointLabel: use.endpoint_label,
      channelIds: use.channels,
      heldBy: use.held_by,
    })),
  };
}

export function probeFromIpc(raw: RawProbe): ProbeReport {
  return {
    ok: raw.ok,
    steps: raw.steps.map(({ name, ok, detail, millis }) => ({ name, ok, detail, millis })),
    summary: raw.summary,
  };
}

export function notificationResultFromIpc(raw: RawNotification): NotificationResult {
  return { target: raw.target, ok: raw.ok, detail: raw.detail };
}

/** The `notifications` section of the bootstrap document (configuration.md §3), as far as the console shows it. */
export interface BootstrapNotifications {
  rate_limit_minutes?: number;
  email?: {
    host: string;
    port: number;
    security?: "starttls" | "tls" | "none";
    username?: string | null;
    password?: { secret: string } | null;
    from: string;
    to: string[];
    severities?: string[];
  } | null;
  webhook?: {
    url: string;
    bearer?: { secret: string } | null;
    severities?: string[];
    timeout_seconds?: number;
  } | null;
}

const DEFAULT_SEVERITIES = ["warning", "critical"];

/** The bootstrap's notification targets with their defaults filled in; null when the section is absent. */
export function notificationSettingsFrom(
  raw: BootstrapNotifications | null | undefined,
): NotificationSettings | null {
  if (raw === null || raw === undefined) return null;
  const { email, webhook } = raw;
  return {
    rateLimitMinutes: raw.rate_limit_minutes ?? 15,
    email: email
      ? {
          host: email.host,
          port: email.port,
          security: email.security ?? "starttls",
          username: email.username ?? null,
          passwordSecret: email.password?.secret ?? null,
          from: email.from,
          to: email.to,
          severities: email.severities ?? DEFAULT_SEVERITIES,
        }
      : null,
    webhook: webhook
      ? {
          url: webhook.url,
          bearerSecret: webhook.bearer?.secret ?? null,
          severities: webhook.severities ?? DEFAULT_SEVERITIES,
          timeoutSeconds: webhook.timeout_seconds ?? 10,
        }
      : null,
  };
}

/**
 * The engine names the problems of a probe's definition relative to the endpoint
 * (`settings.port_name`); this gives them the console's field paths at `endpoints[0]`, the
 * endpoint of `document`, which holds only that definition. Anything else passes through.
 */
export function atEndpointZero(error: unknown, document: WireConfig): unknown {
  if (typeof error !== "object" || error === null || !("fieldErrors" in error)) return error;
  const { fieldErrors } = error as EngineError;
  return {
    ...error,
    fieldErrors: fieldErrors.map(({ field, message }) => ({
      field: field.startsWith("endpoints[") ? field : toUiPath(`endpoints[0].${field}`, document),
      message,
    })),
  };
}
