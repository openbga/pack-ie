import { Link } from "react-aria-components";
import { useEngineQuery } from "@/api/hooks";
import type { Alert as EngineAlert } from "@/api/types";
import { Alert } from "@/components/Alert";
import { QueryContent } from "@/components/QueryContent";
import { Section } from "@/components/Section";
import { StatusBadge } from "@/components/StatusBadge";
import { Timestamp } from "@/components/Timestamp";
import styles from "@/styles/layout.module.css";
import { listedTargets, NotificationTargets } from "./NotificationsTabTargets";
import { NotificationTestSection } from "./NotificationsTabTest";

/** How often the status re-reads the engine's alerts. */
const ALERTS_REFRESH_MS = 5_000;

function Failing({ alert }: { alert: EngineAlert }) {
  return (
    <Alert tone="warning" title={alert.title}>
      <div className={styles.stack}>
        {alert.detail !== "" && <p className={styles.code}>{alert.detail}</p>}
        <p>
          First failed send: <Timestamp value={alert.firstSeenAt} seconds />
        </p>
        <p>{alert.nextAction}</p>
        <p>
          <Link href="/alerts">Open Alerts</Link>
        </p>
      </div>
    </Alert>
  );
}

/**
 * Settings → Notifications (docs/spec/diagnostics.md section 6.1): whether the engine's alert
 * messages are getting through, the targets the bootstrap configures, and a test message.
 */
export function NotificationsTab() {
  const alerts = useEngineQuery(["listAlerts", "open"], (api) => api.listAlerts("open"), {
    refetchInterval: ALERTS_REFRESH_MS,
  });
  const settings = useEngineQuery(["getEngineSettings"], (api) => api.getEngineSettings());
  const secrets = useEngineQuery(["listSecrets"], (api) => api.listSecrets());
  return (
    <div className={styles.stack}>
      <Section title="Status">
        <QueryContent query={alerts} what="notification status">
          {(open) => {
            const failing = open.find((each) => each.kind === "notifications_failing");
            return failing ? (
              <Failing alert={failing} />
            ) : (
              <>
                <StatusBadge tone="success">No target is failing</StatusBadge>
                <p className={styles.hint}>
                  A target is failing from its first failed send until a later send to it succeeds.
                </p>
              </>
            );
          }}
        </QueryContent>
      </Section>
      <NotificationTargets settings={settings} secrets={secrets} />
      <NotificationTestSection
        targets={listedTargets(settings.data?.notifications)}
        isLoading={settings.isPending}
      />
    </div>
  );
}
