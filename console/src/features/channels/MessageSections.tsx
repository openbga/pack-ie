import { Link } from "react-router";
import type { AttentionState, ChannelSnapshot, DestinationStatus } from "@/api/types";
import { Alert } from "@/components/Alert";
import { Button } from "@/components/Button";
import { Facts } from "@/components/Facts";
import { Section } from "@/components/Section";
import { StatusBadge } from "@/components/StatusBadge";
import { Timestamp } from "@/components/Timestamp";
import { formatAge, formatCount } from "@/lib/format";
import { deliveryStatus, protocolLabel } from "@/lib/status";
import { Countdown } from "../shared-operate/Countdown";
import { EndpointIndicator } from "../shared-operate/EndpointIndicator";
import { sourceSessionLabel } from "../shared-operate/stateShapes";
import styles from "./ChannelSections.module.css";

const NOT_REPORTED = <span className={styles.muted}>Not reported</span>;

const ago = (at: string | null, none: string) => (at === null ? none : formatAge(at));

// ── Source connector ─────────────────────────────────────────────────────────────────────

/** The source connector: endpoint, protocol, session, last received and what became of each message. */
export function SourceSection({ channel }: { channel: ChannelSnapshot }) {
  const source = channel.source;
  const [ref] = channel.endpoints;
  if (source === null) {
    return (
      <Section title="Source connector">
        <p className={styles.muted}>
          Not reported: the engine does not report this channel's source connector.
        </p>
      </Section>
    );
  }
  const { endpoint } = source;
  return (
    <Section title="Source connector">
      <Facts
        layout="compact"
        items={[
          {
            label: "Endpoint",
            value: (
              <EndpointIndicator
                label={endpoint.label || ref?.label || endpoint.endpointId}
                state={endpoint.state}
                href={`/endpoints/${endpoint.endpointId}`}
                note={
                  endpoint.state === "listening" && endpoint.activePeer === null
                    ? "waiting for peer"
                    : null
                }
              />
            ),
          },
          { label: "Protocol", value: protocolLabel(source.protocol) },
          { label: "Session", value: sourceSessionLabel(source.session) },
          { label: "Active peer", value: endpoint.activePeer ?? "None" },
          {
            label: "Last received",
            value:
              source.lastReceivedAt === null ? (
                "Nothing received yet"
              ) : (
                <>
                  {formatAge(source.lastReceivedAt)} (
                  <Timestamp value={source.lastReceivedAt} seconds />)
                </>
              ),
          },
          { label: "Received", value: formatCount(source.received) },
          { label: "Duplicates", value: formatCount(source.duplicates) },
          { label: "Invalid", value: formatCount(source.invalid) },
          { label: "Filtered", value: formatCount(source.filtered) },
          { label: "Aborted", value: formatCount(source.aborted) },
        ]}
      />
    </Section>
  );
}

// ── Destinations ─────────────────────────────────────────────────────────────────────────

/** In the order an operator reads them: what the destination refused, then what is uncertain. */
const ATTENTION_STATES: readonly AttentionState[] = [
  "rejected",
  "failed",
  "timed_out",
  "outcome_unknown",
];

function unresolvedCount(destination: DestinationStatus): number {
  return ATTENTION_STATES.reduce((sum, state) => sum + destination.unresolved[state], 0);
}

function Unresolved({ destination }: { destination: DestinationStatus }) {
  const total = unresolvedCount(destination);
  if (total === 0) return <>None</>;
  const parts = ATTENTION_STATES.filter((state) => destination.unresolved[state] > 0).map(
    (state) => {
      const text = `${formatCount(destination.unresolved[state])} ${deliveryStatus(state).label.toLowerCase()}`;
      return state === "failed" && destination.transformErrors > 0
        ? `${text} (${formatCount(destination.transformErrors)} in the mapping)`
        : text;
    },
  );
  return (
    <>
      <StatusBadge tone="danger">{`${formatCount(total)} need attention`}</StatusBadge>{" "}
      <span className={styles.muted}>{parts.join(" · ")}</span>
    </>
  );
}

function nextAttempt(destination: DestinationStatus) {
  if (destination.nextAttemptAt !== null) return <Countdown until={destination.nextAttemptAt} />;
  return destination.enabled ? "None scheduled" : "None: the destination is disabled";
}

/**
 * Failed deliveries whose mapping failed were never sent: the destination is not at fault.
 * The way out is the mapping, then a resend.
 */
function MappingFailures({
  destination,
  channelId,
}: {
  destination: DestinationStatus;
  channelId: string;
}) {
  if (destination.transformErrors === 0) return null;
  const messages = `/messages?channel=${encodeURIComponent(channelId)}&delivery=failed&range=any`;
  return (
    <p>
      <strong>{formatCount(destination.transformErrors)} of them:</strong> the mapping failed — fix
      the mapping, then resend.{" "}
      <Link to={`/channels/${encodeURIComponent(channelId)}/edit`}>Edit the mapping</Link>
      {" · "}
      <Link to={messages}>View failed deliveries</Link>
    </p>
  );
}

function DestinationBlock({
  destination,
  channelId,
}: {
  destination: DestinationStatus;
  channelId: string;
}) {
  const { endpoint } = destination;
  const waiting = destination.queued + destination.sending;
  return (
    <section aria-label={`Destination ${destination.label}`} className={styles.destination}>
      <div className={styles.destinationHead}>
        <h3 className={styles.destinationTitle}>{destination.label}</h3>
        {!destination.enabled && <StatusBadge tone="neutral">Disabled</StatusBadge>}
        <EndpointIndicator
          label={endpoint.label}
          state={endpoint.state}
          href={`/endpoints/${endpoint.endpointId}`}
        />
      </div>
      {!destination.enabled && (
        <p className={styles.muted}>
          {waiting > 0
            ? `Disabled: ${formatCount(waiting)} queued ${waiting === 1 ? "delivery waits" : "deliveries wait"} until it is enabled, and no new messages are queued for it.`
            : "Disabled: no new messages are queued for it."}
        </p>
      )}
      <Facts
        layout="compact"
        items={[
          { label: "Enabled", value: destination.enabled ? "Yes" : "No" },
          { label: "Protocol", value: protocolLabel(destination.protocol) },
          { label: "Queued", value: formatCount(destination.queued) },
          { label: "Sending", value: formatCount(destination.sending) },
          { label: "Unresolved", value: <Unresolved destination={destination} /> },
          {
            label: "Oldest queued",
            value:
              destination.oldestQueuedAt === null
                ? "Nothing waiting"
                : formatAge(destination.oldestQueuedAt),
          },
          { label: "Next attempt", value: nextAttempt(destination) },
          { label: "Last attempt", value: ago(destination.lastAttemptAt, "None yet") },
          { label: "Last acknowledged", value: ago(destination.lastAcknowledgedAt, "Never") },
          {
            label: "Acknowledged since start",
            value: formatCount(destination.counters.acknowledged),
          },
        ]}
      />
      <MappingFailures destination={destination} channelId={channelId} />
      {destination.lastError && (
        <Alert tone={unresolvedCount(destination) > 0 ? "danger" : "info"} title="Last error">
          <p>{destination.lastError.summary}</p>
          {destination.lastError.nextAction !== "" && (
            <p>
              <strong>Next step:</strong> {destination.lastError.nextAction}
            </p>
          )}
        </Alert>
      )}
    </section>
  );
}

/** The queue of a message channel and every destination's deliveries (MSG-03/04). */
export function DestinationsSection({
  channel,
  destinations,
}: {
  channel: ChannelSnapshot;
  destinations: readonly DestinationStatus[];
}) {
  const queue = channel.queue;
  const attention = queue?.needsAttention ?? 0;
  const messages = `/messages?channel=${encodeURIComponent(channel.id)}`;
  const mappingFailures = destinations.reduce((sum, d) => sum + d.transformErrors, 0);
  return (
    <Section
      title="Destinations and deliveries"
      actions={
        <div className={styles.actions}>
          <Button href={messages}>View messages</Button>
          <Button href={`/transmission?channel=${encodeURIComponent(channel.id)}`}>
            Transmission report…
          </Button>
          <Button href={`${messages}&quick=attention&range=any`}>
            {attention > 0
              ? `View unresolved deliveries (${formatCount(attention)})`
              : "View unresolved deliveries"}
          </Button>
        </div>
      }
    >
      <Facts
        layout="compact"
        items={[
          {
            label: "Not yet delivered",
            value: queue === null ? NOT_REPORTED : formatCount(queue.depth),
          },
          {
            label: "Need attention",
            value:
              queue === null ? (
                NOT_REPORTED
              ) : attention > 0 ? (
                <>
                  <StatusBadge tone="danger">{`${formatCount(attention)} need attention`}</StatusBadge>
                  {mappingFailures > 0 && (
                    <span className={styles.muted}>
                      {` · ${formatCount(mappingFailures)} failed in the mapping, never sent`}
                    </span>
                  )}
                </>
              ) : (
                "None"
              ),
          },
          {
            label: "Oldest queued",
            value:
              queue === null
                ? NOT_REPORTED
                : queue.oldestQueuedAt === null
                  ? "Nothing waiting"
                  : formatAge(queue.oldestQueuedAt),
          },
        ]}
      />
      {destinations.length === 0 ? (
        <p className={styles.muted}>
          {channel.source === null
            ? "Not reported: the engine does not report this channel's destinations."
            : "This channel has no destinations."}
        </p>
      ) : (
        <div className={styles.destinations}>
          {destinations.map((destination) => (
            <DestinationBlock
              key={destination.destinationId}
              destination={destination}
              channelId={channel.id}
            />
          ))}
        </div>
      )}
    </Section>
  );
}
