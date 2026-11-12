import type { ComponentProps } from "react";
import type { AuditEvent, EngineError } from "@/api/types";
import { ErrorState } from "@/components/ErrorState";
import { Facts } from "@/components/Facts";
import { SidePanel } from "@/components/SidePanel";
import { Timestamp } from "@/components/Timestamp";
import layout from "@/styles/layout.module.css";
import styles from "./AuditEventPanel.module.css";
import { ObjectCell, OutcomeBadge } from "./auditColumns";
import { ACTION_LABELS, detailText, humanizeKey, ORIGIN_LABELS } from "./auditLabels";

/** The audit panel: an event's details can hold operator text, so it closes when the console goes idle. */
function PanelFrame(props: Omit<ComponentProps<typeof SidePanel>, "closesWhenIdle">) {
  return <SidePanel {...props} closesWhenIdle />;
}

interface AuditEventPanelProps {
  event: AuditEvent;
  onClose: () => void;
  /** The next newer event of the list; absent at the top or when the event is not in the list. */
  onPrevious: (() => void) | undefined;
  /** The next older event of the list; absent at the end of what is loaded. */
  onNext: (() => void) | undefined;
}

/**
 * One event with everything the record holds, beside the list: who, from where, what, to what and
 * how it ended, the hashes that chain it to its neighbours, and its details as field and value
 * rows. The details can hold operator text, so the panel closes when the console goes idle.
 */
export function AuditEventPanel({ event, onClose, onPrevious, onNext }: AuditEventPanelProps) {
  const details = Object.entries(event.details);
  return (
    <PanelFrame
      title={`Event ${event.seq}`}
      onClose={onClose}
      navigation={{
        previousLabel: "Previous event (newer)",
        nextLabel: "Next event (older)",
        onPrevious,
        onNext,
      }}
    >
      <Facts
        items={[
          { label: "Sequence", value: event.seq },
          { label: "Time", value: <Timestamp value={event.at} millis /> },
          {
            label: "Actor",
            value: (
              <>
                {event.actor}
                {event.actorId !== "" && (
                  <>
                    {" "}
                    <code className={layout.code}>{event.actorId}</code>
                  </>
                )}
              </>
            ),
          },
          { label: "Origin", value: ORIGIN_LABELS[event.origin] },
          {
            label: "Action",
            value: (
              <>
                {ACTION_LABELS[event.action]} <code className={layout.code}>{event.action}</code>
              </>
            ),
          },
          {
            label: "Object",
            value: event.objectType === null ? "None" : <ObjectCell event={event} />,
          },
          { label: "Outcome", value: <OutcomeBadge event={event} /> },
          { label: "Hash", value: <code className={layout.code}>{event.hash}</code> },
          { label: "Previous hash", value: <code className={layout.code}>{event.prevHash}</code> },
        ]}
      />
      <h3 className={styles.detailsTitle}>Details</h3>
      {details.length === 0 ? (
        <p className={layout.hint}>This event has no details.</p>
      ) : (
        <dl className={styles.details}>
          {details.map(([key, value]) => (
            <div key={key} className={styles.detail}>
              <dt>
                <span>{humanizeKey(key)}</span> <code className={layout.code}>{key}</code>
              </dt>
              <dd>{detailText(value)}</dd>
            </div>
          ))}
        </dl>
      )}
    </PanelFrame>
  );
}

/** The panel of an event that is being read, because it is not among those loaded. */
export function AuditEventLoading({ seq, onClose }: { seq: number; onClose: () => void }) {
  return (
    <PanelFrame title={`Event ${seq}`} onClose={onClose}>
      <p role="status">Loading event {seq}…</p>
    </PanelFrame>
  );
}

/** The event in the address is not in the log, or could not be read. */
export function AuditEventMissing({
  seq,
  error,
  onRetry,
  onClose,
}: {
  seq: number;
  error: EngineError | null;
  onRetry: () => void;
  onClose: () => void;
}) {
  return (
    <PanelFrame title={`Event ${seq}`} onClose={onClose}>
      {error ? (
        <ErrorState title={`Event ${seq} could not be loaded`} error={error} onRetry={onRetry} />
      ) : (
        <p>
          There is no event {seq} in the audit log. Retention removes whole old files, so older
          events may be gone.
        </p>
      )}
    </PanelFrame>
  );
}
