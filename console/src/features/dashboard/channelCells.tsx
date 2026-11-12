import type { ReactNode } from "react";
import type { ChannelEndpointStatus, ChannelSnapshot, EndpointRef } from "@/api/types";
import { StatusBadge } from "@/components/StatusBadge";
import { formatAge, formatBytes, formatCount } from "@/lib/format";
import { ChannelStateChip } from "../shared-operate/ChannelStateChip";
import { Countdown } from "../shared-operate/Countdown";
import { bufferFill, fullestBuffer } from "../shared-operate/channelRules";
import { EndpointIndicator, type FlowArrow } from "../shared-operate/EndpointIndicator";
import { endpointShape, sessionLabel, sourceSessionLabel } from "../shared-operate/stateShapes";
import { useNow } from "../shared-operate/useNow";
import { acknowledgedCounts, acknowledgedTimes } from "./acknowledged";
import styles from "./ChannelTable.module.css";
import type { ChannelRow } from "./rank";

const Muted = ({ children }: { children: ReactNode }) => (
  <span className={styles.muted}>{children}</span>
);

const Lines = ({ children }: { children: ReactNode }) => (
  <span className={styles.lines}>{children}</span>
);

// ── Channel and state ────────────────────────────────────────────────────────────────────

export function ChannelCell({ row }: { row: ChannelRow }) {
  const { channel } = row;
  return (
    <>
      {row.critical && <span className={styles.critical} />}
      {channel.label}{" "}
      <span className={styles.kind}>{channel.kind === "bridge" ? "Bridge" : "Message"}</span>
    </>
  );
}

/** The state, plus the words that say a critical alert is open (the row's tint is not the only signal). */
export function StateCell({ row }: { row: ChannelRow }) {
  return (
    <span className={styles.badges}>
      <ChannelStateChip channel={row.channel} />
      {row.critical && <StatusBadge tone="danger">Critical alert</StatusBadge>}
    </span>
  );
}

// ── Endpoints ────────────────────────────────────────────────────────────────────────────

interface Side {
  label: string;
  state: EndpointRef["state"];
  waitingForPeer: boolean;
}

function sideOf(status: ChannelEndpointStatus | undefined, ref: EndpointRef | undefined): Side {
  const state = status?.state ?? ref?.state ?? "disconnected";
  return {
    label: status?.label ?? ref?.label ?? "Not set",
    state,
    waitingForPeer: state === "listening" && (status?.activePeer ?? null) === null,
  };
}

function arrows(channel: ChannelSnapshot): [FlowArrow | null, FlowArrow | null] {
  const bridge = channel.bridge;
  if (bridge?.connectStrategy !== "receiver_then_sender" || bridge.receiver === null) {
    return [null, null];
  }
  return bridge.receiver === "a" ? ["inbound", "outbound"] : ["outbound", "inbound"];
}

function BridgeEndpoints({ channel }: { channel: ChannelSnapshot }) {
  const [refA, refB] = channel.endpoints;
  const [arrowA, arrowB] = arrows(channel);
  const sides: [string, Side, FlowArrow | null][] = [
    ["A", sideOf(channel.bridge?.endpointA, refA), arrowA],
    ["B", sideOf(channel.bridge?.endpointB, refB), arrowB],
  ];
  return (
    <Lines>
      {sides.map(([role, side, arrow]) => (
        <EndpointIndicator
          key={role}
          side={role}
          label={side.label}
          state={side.state}
          arrow={arrow}
          note={side.waitingForPeer ? "no peer" : null}
        />
      ))}
    </Lines>
  );
}

/** Destinations whose endpoint is not ready first, so a problem is never behind "and 2 more". */
const SHOWN_DESTINATIONS = 3;
const notReady = (ref: EndpointRef) => endpointShape(ref.state).tone !== "neutral";

function MessageEndpoints({ channel }: { channel: ChannelSnapshot }) {
  const [sourceRef, ...destinations] = channel.endpoints;
  const source = sideOf(channel.source?.endpoint, sourceRef);
  const ordered = [...destinations].sort((a, b) => Number(notReady(b)) - Number(notReady(a)));
  const hidden = ordered.length - SHOWN_DESTINATIONS;
  return (
    <Lines>
      <EndpointIndicator
        side="Source"
        label={source.label}
        state={source.state}
        note={source.waitingForPeer ? "no peer" : null}
      />
      {ordered.slice(0, SHOWN_DESTINATIONS).map((destination) => (
        <EndpointIndicator
          key={destination.endpointId}
          side="To"
          label={destination.label}
          state={destination.state}
        />
      ))}
      {hidden > 0 && <Muted>{`and ${hidden} more`}</Muted>}
    </Lines>
  );
}

export function EndpointsCell({ channel }: { channel: ChannelSnapshot }) {
  return channel.kind === "bridge" ? (
    <BridgeEndpoints channel={channel} />
  ) : (
    <MessageEndpoints channel={channel} />
  );
}

// ── Traffic, activity, queue ─────────────────────────────────────────────────────────────

export function TrafficCell({ channel }: { channel: ChannelSnapshot }) {
  if (channel.kind === "bridge") {
    const bridge = channel.bridge;
    if (bridge === null) return <Muted>Not reported</Muted>;
    return (
      <Lines>
        <span>A → B {formatBytes(bridge.bytesAToB)}</span>
        <span>B → A {formatBytes(bridge.bytesBToA)}</span>
      </Lines>
    );
  }
  const acknowledged = acknowledgedCounts(channel);
  return (
    <Lines>
      <span>Received {formatCount(channel.source?.received ?? channel.counters.messagesIn)}</span>
      {acknowledged === null ? (
        <Muted>Acknowledged not reported</Muted>
      ) : (
        <Truncated text={acknowledged} />
      )}
    </Lines>
  );
}

/** One line; the full text is the title. */
function Truncated({ text }: { text: string }) {
  return (
    <span className={styles.oneLine} title={text}>
      {text}
    </span>
  );
}

const ago = (at: string | null, now: number) => (at === null ? "no activity" : formatAge(at, now));

export function ActivityCell({ channel }: { channel: ChannelSnapshot }) {
  const now = useNow();
  if (channel.kind === "message") {
    const received = channel.source?.lastReceivedAt ?? channel.lastActivityAt;
    const acknowledged = acknowledgedTimes(channel, now);
    return (
      <Lines>
        <span>Received {ago(received, now)}</span>
        {acknowledged === null ? (
          <Muted>Acknowledged not reported</Muted>
        ) : (
          <Truncated text={acknowledged} />
        )}
      </Lines>
    );
  }
  const a = channel.bridge?.endpointA.lastActivityAt ?? null;
  const b = channel.bridge?.endpointB.lastActivityAt ?? null;
  if (channel.bridge === null || (a === null && b === null)) {
    return channel.lastActivityAt === null ? (
      <Muted>No activity yet</Muted>
    ) : (
      <span>{formatAge(channel.lastActivityAt, now)}</span>
    );
  }
  return (
    <Lines>
      <span>A {ago(a, now)}</span>
      <span>B {ago(b, now)}</span>
    </Lines>
  );
}

export function QueueCell({ row }: { row: ChannelRow }) {
  const { channel } = row;
  if (channel.kind === "message") {
    if (channel.queue === null) return <Muted>Not reported</Muted>;
    return (
      <Lines>
        <span>{formatCount(channel.queue.depth)} queued</span>
        {row.unresolved > 0 ? (
          <StatusBadge tone="warning">{`${formatCount(row.unresolved)} unresolved`}</StatusBadge>
        ) : (
          <Muted>0 unresolved</Muted>
        )}
      </Lines>
    );
  }
  const fullest = fullestBuffer(channel.buffers);
  if (fullest === undefined) return <Muted>Not reported</Muted>;
  const overflowing = channel.buffers.some((buffer) => buffer.overflowing);
  const episodes = channel.bridge?.overflowCount ?? 0;
  return (
    <span className={styles.badges}>
      <span>{Math.round(bufferFill(fullest) * 100)}% full</span>
      {overflowing && <StatusBadge tone="danger">Overflow</StatusBadge>}
      {!overflowing && episodes > 0 && (
        <Muted>{`${episodes} overflow${episodes === 1 ? "" : "s"}`}</Muted>
      )}
    </span>
  );
}

// ── Session, retry, error ────────────────────────────────────────────────────────────────

export function SessionCell({ channel }: { channel: ChannelSnapshot }) {
  if (channel.bridge !== null) return <span>{sessionLabel(channel.bridge.sessionState)}</span>;
  if (channel.source !== null) return <span>{sourceSessionLabel(channel.source.session)}</span>;
  return <Muted>Not reported</Muted>;
}

export function RetryCell({ channel }: { channel: ChannelSnapshot }) {
  const { retries } = channel.counters;
  if (channel.nextRetryAt !== null) {
    return (
      <span>
        {retries} · <Countdown until={channel.nextRetryAt} />
      </span>
    );
  }
  return retries > 0 ? <span>{retries}</span> : <Muted>None</Muted>;
}

export function ErrorCell({ channel }: { channel: ChannelSnapshot }) {
  const summary = channel.lastError?.summary ?? channel.recentErrors[0]?.summary ?? null;
  if (summary === null) return <Muted>None</Muted>;
  return (
    <span className={styles.clamp} title={summary}>
      {summary}
    </span>
  );
}
