import type {
  ChannelSnapshot,
  DestinationStatus,
  EndpointSnapshot,
  EndpointState,
  LastError,
} from "@/api/types";
import { parseErrorSummary } from "./errorSummary";

/** Where a channel's last error came from, and what that part of the channel is doing about it. */
export type ErrorOrigin =
  | {
      kind: "destination";
      destination: DestinationStatus;
      /** The destination endpoint's failure streak; null when its snapshot is not known. */
      streak: number | null;
      /** The destination's next attempt, else its endpoint's next retry. */
      nextAttemptAt: string | null;
    }
  | { kind: "source"; label: string; streak: number }
  | { kind: "bridge"; streak: number };

const FAILING: ReadonlySet<EndpointState> = new Set(["error", "retrying", "disconnected"]);

/** The destination is enabled and its endpoint is down, so its deliveries are being retried. */
export function isRetrying(destination: DestinationStatus): boolean {
  return (
    destination.enabled &&
    destination.nextAttemptAt !== null &&
    (FAILING.has(destination.endpoint.state) || destination.lastError !== null)
  );
}

function destinationOrigin(
  destination: DestinationStatus,
  endpoints: readonly EndpointSnapshot[],
): ErrorOrigin {
  const endpoint = endpoints.find((candidate) => candidate.id === destination.endpoint.endpointId);
  return {
    kind: "destination",
    destination,
    streak: endpoint?.consecutiveFailureCount ?? null,
    nextAttemptAt: destination.nextAttemptAt ?? endpoint?.nextRetryAt ?? null,
  };
}

function bridgeOrigin(channel: ChannelSnapshot, role: string | undefined): ErrorOrigin | null {
  const bridge = channel.bridge;
  if (bridge === null) return null;
  const side = role === "A" ? bridge.endpointA : role === "B" ? bridge.endpointB : null;
  const streaks = (side === null ? [bridge.endpointA, bridge.endpointB] : [side]).map(
    (candidate) => candidate.consecutiveFailureCount,
  );
  return { kind: "bridge", streak: Math.max(...streaks) };
}

/**
 * The part of the channel the error belongs to. The engine's summary names it (`source`,
 * `A`, `B`, `destination <id>`); a summary in other words is matched to the destination
 * that reports the same error, else the destination whose endpoint is down while the source
 * is healthy. Null when the channel gives no way to tell.
 */
export function resolveErrorOrigin(
  channel: ChannelSnapshot,
  error: LastError,
  destinations: readonly DestinationStatus[],
  endpoints: readonly EndpointSnapshot[],
): ErrorOrigin | null {
  const role = parseErrorSummary(error.summary)?.role;
  if (channel.kind === "bridge") return bridgeOrigin(channel, role);
  const source = channel.source;
  const sourceOrigin: ErrorOrigin | null =
    source === null
      ? null
      : {
          kind: "source",
          label: source.endpoint.label,
          streak: source.endpoint.consecutiveFailureCount,
        };
  if (role === "source") return sourceOrigin;
  const named = destinations.find((d) => role === `destination ${d.destinationId}`);
  const reporting = destinations.find((d) => d.lastError?.summary === error.summary);
  const sourceDown = source !== null && FAILING.has(source.endpoint.state);
  const down = sourceDown
    ? undefined
    : destinations.find((d) => d.enabled && FAILING.has(d.endpoint.state));
  const destination = named ?? reporting ?? down;
  return destination === undefined ? sourceOrigin : destinationOrigin(destination, endpoints);
}
