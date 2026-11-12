import { Link } from "react-aria-components";
import type { LogEvent } from "@/api/types";
import { Facts } from "@/components/Facts";
import { SidePanel } from "@/components/SidePanel";
import { StatusBadge } from "@/components/StatusBadge";
import { Timestamp } from "@/components/Timestamp";
import { logLevelTone } from "@/lib/status";
import styles from "./LogEventPanel.module.css";
import type { LogLabels } from "./logColumns";
import { LEVEL_LABELS } from "./logLevels";

interface LogEventPanelProps {
  event: LogEvent;
  labels: LogLabels;
  onClose: () => void;
  /** The next newer event in the list; absent at the top. */
  onPrevious: (() => void) | undefined;
  /** The next older event in the list; absent at the end. */
  onNext: (() => void) | undefined;
}

/**
 * One event with everything the engine logged for it, beside the list: the full message, the
 * time to the millisecond, its event name, target and sequence, its structured fields and links
 * to the channel, endpoint and message it names. Nonmodal, so the list stays usable and keeps focus; Escape closes it from inside.
 */
export function LogEventPanel({ event, labels, onClose, onPrevious, onNext }: LogEventPanelProps) {
  const fields = Object.entries(event.fields);
  return (
    <SidePanel
      title={`Event ${event.seq}`}
      badge={
        <StatusBadge tone={logLevelTone(event.level)}>{LEVEL_LABELS[event.level]}</StatusBadge>
      }
      onClose={onClose}
      navigation={{ previousLabel: "Previous", nextLabel: "Next", onPrevious, onNext }}
      size="narrow"
    >
      <p className={styles.message}>{event.message}</p>
      <Facts
        items={[
          { label: "Time", value: <Timestamp value={event.at} millis /> },
          {
            label: "Event",
            value: event.event === null ? "None" : <code>{event.event}</code>,
          },
          { label: "Target", value: <code>{event.target}</code> },
          { label: "Sequence", value: event.seq },
          {
            label: "Channel",
            value: <Named id={event.channelId} href="/channels" names={labels.channels} />,
          },
          {
            label: "Endpoint",
            value: <Named id={event.endpointId} href="/endpoints" names={labels.endpoints} />,
          },
          { label: "Message", value: <Named id={event.messageId} href="/messages" /> },
        ]}
      />
      <h3 className={styles.fieldsTitle}>Fields</h3>
      {fields.length === 0 ? (
        <p className={styles.muted}>This event has no extra fields.</p>
      ) : (
        <div className={styles.fields}>
          <Facts items={fields.map(([name, value]) => ({ label: name, value }))} />
        </div>
      )}
    </SidePanel>
  );
}

/** What an event names, as a link to its page; "None" when it names nothing. */
function Named({
  id,
  href,
  names,
}: {
  id: string | null;
  href: string;
  names?: ReadonlyMap<string, string>;
}) {
  if (id === null) return "None";
  return <Link href={`${href}/${encodeURIComponent(id)}`}>{names?.get(id) ?? id}</Link>;
}
