import { useEngineQuery } from "@/api/hooks";
import type { Alert, LogEvent, RecentError } from "@/api/types";
import { Button } from "@/components/Button";
import { ErrorState } from "@/components/ErrorState";
import { LoadingState } from "@/components/LoadingState";
import { type MiniColumn, MiniTable } from "@/components/MiniTable";
import { Section } from "@/components/Section";
import { StatusBadge } from "@/components/StatusBadge";
import { Timestamp } from "@/components/Timestamp";
import { alertSeverityLabel, alertTone, logLevelTone } from "@/lib/status";
import styles from "./ChannelSections.module.css";

/** How many events the page shows; the full history is in Logs. */
const EVENTS = 20;

const EVENT_COLUMNS: readonly MiniColumn<LogEvent>[] = [
  { id: "time", header: "Time", cell: (event) => <Timestamp value={event.at} seconds /> },
  {
    id: "level",
    header: "Level",
    cell: (event) => <StatusBadge tone={logLevelTone(event.level)}>{event.level}</StatusBadge>,
  },
  { id: "message", header: "Event", cell: (event) => event.message },
];

/** The channel's newest log events (state changes, errors, commands) and a way into Logs. */
export function RecentEventsSection({ channelId }: { channelId: string }) {
  const events = useEngineQuery(
    ["queryLogs", { channelId }, "recent"],
    (api) => api.queryLogs({ channelId }, { before: null, limit: EVENTS }),
    { refetchInterval: 5_000 },
  );
  return (
    <Section
      title="Recent events"
      actions={
        <Button href={`/logs?channel=${encodeURIComponent(channelId)}`}>View in Logs</Button>
      }
    >
      {events.isPending ? (
        <LoadingState label="Loading events…" rows={3} />
      ) : events.isLoadingError ? (
        <ErrorState
          title="The events could not be loaded"
          error={events.error}
          onRetry={() => void events.refetch()}
        />
      ) : events.data.events.length === 0 ? (
        <p className={styles.muted}>No events are recorded for this channel yet.</p>
      ) : (
        <MiniTable
          label="Recent events of this channel"
          columns={EVENT_COLUMNS}
          rows={events.data.events}
          getRowId={(event) => String(event.seq)}
        />
      )}
    </Section>
  );
}

const ERROR_COLUMNS: readonly MiniColumn<RecentError>[] = [
  { id: "time", header: "Time", cell: (error) => <Timestamp value={error.at} seconds /> },
  { id: "summary", header: "Error", cell: (error) => error.summary },
];

/** The errors the snapshot keeps for the channel, newest first. */
export function RecentErrorsSection({ errors }: { errors: readonly RecentError[] }) {
  return (
    <Section title="Recent errors">
      {errors.length === 0 ? (
        <p className={styles.muted}>No recent errors.</p>
      ) : (
        <MiniTable
          label="Recent errors of this channel"
          columns={ERROR_COLUMNS}
          rows={errors}
          getRowId={(error) => `${error.at}|${error.summary}`}
        />
      )}
    </Section>
  );
}

/** Open alerts that concern the channel, each with its next step. */
export function ChannelAlertsSection({ alerts }: { alerts: readonly Alert[] }) {
  return (
    <Section title="Active alerts" actions={<Button href="/alerts">View alerts</Button>}>
      {alerts.length === 0 ? (
        <p className={styles.muted}>No active alerts for this channel.</p>
      ) : (
        <ul className={styles.events} aria-label="Active alerts of this channel">
          {alerts.map((alert) => (
            <li key={alert.id}>
              <StatusBadge tone={alertTone(alert.severity)}>
                {alertSeverityLabel(alert.severity)}
              </StatusBadge>{" "}
              <strong>{alert.title}</strong>
              {alert.detail !== "" && <span className={styles.muted}> · {alert.detail}</span>}
              <div>
                <strong>Next step:</strong> {alert.nextAction}
              </div>
            </li>
          ))}
        </ul>
      )}
    </Section>
  );
}
