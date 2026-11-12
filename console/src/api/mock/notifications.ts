import type { NotificationResult, NotificationSettings } from "../types";
import type { AlertCondition } from "./alerts";
import { MOCK_NOTIFICATIONS } from "./settings";
import { iso } from "./util";
import type { MockWorld } from "./world";

export type NotificationTarget = "email" | "webhook";

/** What the console can read of the bootstrap, and which targets the engine has. */
export type NotificationsMode = "both" | "none" | "unknown";

const TARGETS: readonly NotificationTarget[] = ["email", "webhook"];
const DEFAULT_ERROR = "connection refused";
// Verbatim from the catalog in docs/spec/diagnostics.md section 6.
const NEXT_ACTION =
  "Check the notification settings (host, port, security, credentials secret, webhook address) and that this computer can reach the target, then send a test message (`packie notify test`). The alert clears after the next successful delivery to every target.";

/**
 * The mock's notification targets (docs/spec/diagnostics.md section 6.1): which of them the
 * engine has, which fail and since when. A target is failing from its first failed send until
 * a later send to it succeeds; only test messages are sent here.
 */
export class MockNotifications {
  private mode: NotificationsMode = "both";
  /** Targets a journey made fail, with the error their sends end in. */
  private readonly broken = new Map<NotificationTarget, string>();
  private readonly failing = new Map<NotificationTarget, { sinceMs: number; error: string }>();

  constructor(private readonly now: () => number) {}

  setMode(mode: NotificationsMode): void {
    this.mode = mode;
  }

  /** The targets the engine has: none when no section is configured, else both. */
  private targets(): readonly NotificationTarget[] {
    return this.mode === "none" ? [] : TARGETS;
  }

  /** The console's reading of the bootstrap: the section, null when it has none, undefined when unreadable. */
  settings(): NotificationSettings | null | undefined {
    if (this.mode === "unknown") return undefined;
    return this.mode === "none" ? null : MOCK_NOTIFICATIONS;
  }

  /** Journey hook: sends to `target` fail with `error` from now on (the target is failing at once); null repairs them all. */
  breakTarget(target: NotificationTarget | null, error = DEFAULT_ERROR): void {
    if (target === null) {
      this.broken.clear();
      return;
    }
    this.broken.set(target, error);
    this.failing.set(target, { sinceMs: this.failing.get(target)?.sinceMs ?? this.now(), error });
  }

  /** One test message to every target the engine has. */
  sendTest(): NotificationResult[] {
    return this.targets().map((target) => {
      const error = this.broken.get(target);
      if (error === undefined) {
        this.failing.delete(target);
        return { target, ok: true, detail: "sent" };
      }
      this.failing.set(target, { sinceMs: this.failing.get(target)?.sinceMs ?? this.now(), error });
      return { target, ok: false, detail: error };
    });
  }

  /** The `notifications_failing` alert while any target the engine has is failing. */
  condition(): AlertCondition | null {
    const failing = this.targets().flatMap((target) => {
      const state = this.failing.get(target);
      return state === undefined ? [] : [{ target, ...state }];
    });
    if (failing.length === 0) return null;
    const since = Math.min(...failing.map((each) => each.sinceMs));
    return {
      key: "notifications_failing",
      kind: "notifications_failing",
      severity: "warning",
      title: `Notifications are failing: ${failing
        .map((each) => `${each.target} (${each.error})`)
        .join(", ")}`,
      detail: `targets: ${failing.map((each) => each.target).join(", ")}, failing since: ${iso(since)}`,
      channelId: null,
      endpointId: null,
      nextAction: NEXT_ACTION,
      sinceMs: since,
    };
  }
}

/**
 * What each notification target answers to a test message; empty when none is configured.
 * Audited like the engine does: nothing is sent while the log cannot be written, and the
 * outcome carries each target's result.
 */
export function sendTestNotifications(world: MockWorld): NotificationResult[] {
  world.audit.record({ action: "notification_test", outcome: "requested" });
  const results = world.notifications.sendTest();
  world.audit.record({
    action: "notification_test",
    details: { results: results.map(({ target, ok }) => ({ target, ok })) },
  });
  world.publish();
  return results;
}
