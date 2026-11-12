import type { ChannelEndpointStatus, ChannelSnapshot, EndpointRef } from "@/api/types";
import { Facts } from "@/components/Facts";
import { type MiniColumn, MiniTable } from "@/components/MiniTable";
import { Section } from "@/components/Section";
import { StatusBadge } from "@/components/StatusBadge";
import { formatAge, formatBytes, formatCount } from "@/lib/format";
import { endpointKindLabel } from "@/lib/status";
import { bufferFill } from "../shared-operate/channelRules";
import { EndpointIndicator, type FlowArrow } from "../shared-operate/EndpointIndicator";
import { PressureMeter } from "../shared-operate/PressureMeter";
import { useNow } from "../shared-operate/useNow";
import styles from "./ChannelSections.module.css";

const NOT_REPORTED = <span className={styles.muted}>Not reported</span>;

// ── Endpoints A and B ────────────────────────────────────────────────────────────────────

interface Side {
  role: "A" | "B";
  ref: EndpointRef | undefined;
  /** Null when the engine reports no per-side status. */
  status: ChannelEndpointStatus | null;
  arrow: FlowArrow | null;
  /** Bytes that came in from this side, and bytes written out to it. */
  received: number | null;
  sent: number | null;
}

function sidesOf(channel: ChannelSnapshot): Side[] {
  const bridge = channel.bridge;
  const [refA, refB] = channel.endpoints;
  const receiver = bridge?.connectStrategy === "receiver_then_sender" ? bridge.receiver : null;
  const arrow = (side: "a" | "b"): FlowArrow | null =>
    receiver === null ? null : receiver === side ? "inbound" : "outbound";
  return [
    {
      role: "A",
      ref: refA,
      status: bridge?.endpointA ?? null,
      arrow: arrow("a"),
      received: bridge?.bytesAToB ?? null,
      sent: bridge?.bytesBToA ?? null,
    },
    {
      role: "B",
      ref: refB,
      status: bridge?.endpointB ?? null,
      arrow: arrow("b"),
      received: bridge?.bytesBToA ?? null,
      sent: bridge?.bytesAToB ?? null,
    },
  ];
}

const bytes = (value: number | null) => (value === null ? NOT_REPORTED : formatBytes(value));

function LastActivity({ at }: { at: string | null }) {
  const now = useNow();
  return at === null ? (
    <span className={styles.muted}>No activity yet</span>
  ) : (
    <span>{formatAge(at, now)}</span>
  );
}

const SIDE_COLUMNS: readonly MiniColumn<Side>[] = [
  {
    id: "endpoint",
    header: "Endpoint",
    cell: (side) => {
      const id = side.status?.endpointId ?? side.ref?.endpointId;
      const state = side.status?.state ?? side.ref?.state;
      if (id === undefined || state === undefined) return NOT_REPORTED;
      return (
        <EndpointIndicator
          side={`Side ${side.role}`}
          label={side.status?.label ?? side.ref?.label ?? id}
          state={state}
          arrow={side.arrow}
          href={`/endpoints/${id}`}
          note={
            state === "listening" && side.status?.activePeer === null ? "waiting for peer" : null
          }
        />
      );
    },
  },
  {
    id: "kind",
    header: "Kind",
    cell: (side) => (side.status === null ? NOT_REPORTED : endpointKindLabel(side.status.kind)),
  },
  {
    id: "peer",
    header: "Active peer",
    cell: (side) =>
      side.status === null
        ? NOT_REPORTED
        : (side.status.activePeer ?? <span className={styles.muted}>None</span>),
  },
  {
    id: "streak",
    header: "Failure streak",
    align: "end",
    cell: (side) =>
      side.status === null ? NOT_REPORTED : formatCount(side.status.consecutiveFailureCount),
  },
  {
    id: "activity",
    header: "Last activity",
    cell: (side) =>
      side.status === null ? NOT_REPORTED : <LastActivity at={side.status.lastActivityAt} />,
  },
  { id: "received", header: "Bytes received", align: "end", cell: (side) => bytes(side.received) },
  { id: "sent", header: "Bytes sent", align: "end", cell: (side) => bytes(side.sent) },
];

/** Side A and side B, each with its endpoint, state, peer, failure streak, activity and bytes. */
export function BridgeEndpointsSection({ channel }: { channel: ChannelSnapshot }) {
  return (
    <Section title="Endpoints">
      <MiniTable
        label="Endpoints of this channel"
        columns={SIDE_COLUMNS}
        rows={sidesOf(channel)}
        getRowId={(side) => side.role}
      />
    </Section>
  );
}

// ── Traffic and buffers ──────────────────────────────────────────────────────────────────

interface Direction {
  id: "a_to_b" | "b_to_a";
  title: string;
  bytes: number | null;
  messages: number | null;
}

const messageCount = (value: number | null) =>
  value === null ? <span className={styles.muted}>No framer</span> : formatCount(value);

const DIRECTION_COLUMNS: readonly MiniColumn<Direction>[] = [
  { id: "direction", header: "Direction", cell: (d) => d.title },
  { id: "bytes", header: "Bytes", align: "end", cell: (d) => bytes(d.bytes) },
  {
    id: "messages",
    header: "Framed messages",
    align: "end",
    cell: (d) => messageCount(d.messages),
  },
];

/** Bytes in each direction, framed message counts when a tap is configured, and the tap's own counts. */
export function BridgeTrafficSection({ channel }: { channel: ChannelSnapshot }) {
  const bridge = channel.bridge;
  const [a, b] = channel.endpoints;
  const nameA = bridge?.endpointA.label ?? a?.label ?? "A";
  const nameB = bridge?.endpointB.label ?? b?.label ?? "B";
  const rows: Direction[] = [
    {
      id: "a_to_b",
      title: `${nameA} → ${nameB}`,
      bytes: bridge?.bytesAToB ?? null,
      messages: bridge?.messagesAToB ?? null,
    },
    {
      id: "b_to_a",
      title: `${nameB} → ${nameA}`,
      bytes: bridge?.bytesBToA ?? null,
      messages: bridge?.messagesBToA ?? null,
    },
  ];
  return (
    <Section title="Traffic">
      <MiniTable
        label="Bytes by direction"
        columns={DIRECTION_COLUMNS}
        rows={rows}
        getRowId={(d) => d.id}
      />
      {bridge?.tap && (
        <Facts
          layout="compact"
          items={[
            { label: "Framer", value: bridge.tap.framer },
            ...(bridge.framingErrors === null
              ? []
              : [{ label: "Framing errors", value: formatCount(bridge.framingErrors) }]),
            ...(bridge.tapDropped === null
              ? []
              : [{ label: "Observations dropped", value: formatCount(bridge.tapDropped) }]),
          ]}
        />
      )}
    </Section>
  );
}

/** Per-direction buffer fill (BRG-03): segments, bytes and limits, and the overflow count. */
export function BridgeBuffersSection({ channel }: { channel: ChannelSnapshot }) {
  const [a, b] = channel.endpoints;
  const names = {
    a_to_b: [
      channel.bridge?.endpointA.label ?? a?.label ?? "A",
      channel.bridge?.endpointB.label ?? b?.label ?? "B",
    ],
    b_to_a: [
      channel.bridge?.endpointB.label ?? b?.label ?? "B",
      channel.bridge?.endpointA.label ?? a?.label ?? "A",
    ],
  };
  const overflowing = channel.buffers.some((buffer) => buffer.overflowing);
  return (
    <Section title="Buffers">
      {channel.buffers.length === 0 ? (
        <p className={styles.muted}>No buffer data yet. The channel has not started.</p>
      ) : (
        <div className={styles.meters}>
          {channel.buffers.map((buffer) => {
            const [from, to] = names[buffer.direction];
            const segments =
              buffer.maxSegments === null
                ? `${formatCount(buffer.segments)} segments`
                : `${formatCount(buffer.segments)} of ${formatCount(buffer.maxSegments)} segments`;
            return (
              <PressureMeter
                key={buffer.direction}
                label={`${from} → ${to}`}
                fraction={bufferFill(buffer)}
                detail={`${formatBytes(buffer.usedBytes)} of ${formatBytes(buffer.capacityBytes)} · ${segments}`}
              />
            );
          })}
        </div>
      )}
      <Facts
        layout="compact"
        items={[
          {
            label: "Overflow episodes",
            value:
              channel.bridge === null ? NOT_REPORTED : formatCount(channel.bridge.overflowCount),
          },
          ...(overflowing
            ? [
                {
                  label: "Now",
                  value: (
                    <StatusBadge tone="danger">
                      Overflowing: the writer waits, nothing is dropped
                    </StatusBadge>
                  ),
                },
              ]
            : []),
        ]}
      />
    </Section>
  );
}
