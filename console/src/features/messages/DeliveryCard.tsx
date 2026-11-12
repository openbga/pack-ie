import type { Delivery } from "@/api/types";
import { Button } from "@/components/Button";
import { Facts } from "@/components/Facts";
import { StatusBadge } from "@/components/StatusBadge";
import { Timestamp } from "@/components/Timestamp";
import { BodyViewer } from "./BodyViewer";
import styles from "./DeliveryCard.module.css";
import {
  ackSentence,
  attemptResultText,
  canClose,
  deliveryRecordStatus,
  isInProgress,
  reasonWithCode,
} from "./messageWording";

interface DeliveryCardProps {
  delivery: Delivery;
  /** Whether the message can be resent at all (a message still being received cannot). */
  canResend: boolean;
  onResend: (delivery: Delivery) => void;
  onClose: (delivery: Delivery) => void;
}

const NONE = <span className={styles.muted}>None</span>;

/**
 * One destination of the message: its state in the spec's words, what its acknowledgement
 * proved (never more than it did), the reason, every attempt, and the transformed message on
 * request. Resend and close act on this delivery.
 */
export function DeliveryCard({ delivery, canResend, onResend, onClose }: DeliveryCardProps) {
  const status = deliveryRecordStatus(delivery.state);
  const titleId = `delivery-${delivery.id}`;
  return (
    <article className={styles.card} aria-labelledby={titleId}>
      <div className={styles.header}>
        <h3 id={titleId} className={styles.title}>
          {delivery.destinationLabel}
        </h3>
        <StatusBadge tone={status.tone}>{status.label}</StatusBadge>
        <div className={styles.actions}>
          {canResend && (
            <Button
              aria-label={`Resend to ${delivery.destinationLabel}`}
              onPress={() => onResend(delivery)}
            >
              Resend…
            </Button>
          )}
          {canClose(delivery.state) && (
            <Button
              aria-label={`Close delivery to ${delivery.destinationLabel}`}
              onPress={() => onClose(delivery)}
            >
              Close delivery…
            </Button>
          )}
        </div>
      </div>
      <Facts layout="compact" items={factsOf(delivery)} />
      <Attempts delivery={delivery} />
      <div>
        <h4 className={styles.heading}>Transformed message</h4>
        {delivery.transformedHash === null ? (
          <p className={styles.muted}>
            {delivery.reasonCode === "transform_error"
              ? "None: the mapping failed, so no output was produced for this destination."
              : "None is stored for this delivery."}
          </p>
        ) : (
          <BodyViewer
            messageId={delivery.messageId}
            which={{ kind: "transformed", deliveryId: delivery.id }}
            name={`Transformed message to ${delivery.destinationLabel}`}
          />
        )}
      </div>
    </article>
  );
}

function factsOf(delivery: Delivery) {
  return [
    { label: "Acknowledgement", value: ackSentence(delivery) },
    {
      label: "Reason",
      value: delivery.reasonCode === null ? NONE : reasonWithCode(delivery.reasonCode),
    },
    ...(delivery.lastError === null ? [] : [{ label: "Last error", value: delivery.lastError }]),
    {
      label: "Closed with reason",
      value: delivery.closedReason ?? NONE,
    },
    ...(delivery.resendReason === null ? [] : [{ label: "Resent", value: delivery.resendReason }]),
    {
      label: "Attempts",
      value: `${delivery.attempts} in this cycle, ${delivery.totalAttempts} in all`,
    },
    {
      label: "Next attempt",
      value:
        delivery.nextAttemptAt === null ? (
          NONE
        ) : (
          <Timestamp value={delivery.nextAttemptAt} seconds />
        ),
    },
    { label: "Created", value: <Timestamp value={delivery.createdAt} seconds /> },
    { label: "Last change", value: <Timestamp value={delivery.updatedAt} seconds /> },
    {
      label: "Mapping version",
      value: delivery.mappingVersion === null ? NONE : `Configuration ${delivery.mappingVersion}`,
    },
  ];
}

function Attempts({ delivery }: { delivery: Delivery }) {
  const { attemptLog } = delivery;
  if (attemptLog.length === 0) {
    return (
      <p className={styles.muted}>
        {isInProgress(delivery.state)
          ? "No attempt has started yet."
          : "No attempt was made: nothing was sent to this destination."}
      </p>
    );
  }
  return (
    <div className={styles.scroll}>
      <table className={styles.attempts} aria-label={`Attempts to ${delivery.destinationLabel}`}>
        <thead>
          <tr>
            <th scope="col">Attempt</th>
            <th scope="col">Started</th>
            <th scope="col">Ended</th>
            <th scope="col">Result</th>
            <th scope="col">Acknowledgement code</th>
            <th scope="col">ACK ID</th>
            <th scope="col">Error code</th>
          </tr>
        </thead>
        <tbody>
          {attemptLog.map((attempt) => (
            <tr key={attempt.attemptNo}>
              <td>{attempt.attemptNo}</td>
              <td>
                <Timestamp value={attempt.startedAt} seconds />
              </td>
              <td>
                {attempt.endedAt === null ? (
                  <span className={styles.muted}>Not yet</span>
                ) : (
                  <Timestamp value={attempt.endedAt} seconds />
                )}
              </td>
              <td>{attemptResultText(attempt.result)}</td>
              <td className={styles.code}>{attempt.ackCode ?? NONE}</td>
              <td className={styles.code}>{attempt.ackControlId ?? NONE}</td>
              <td className={styles.code}>{attempt.errorCode ?? NONE}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
