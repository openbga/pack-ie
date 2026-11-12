import type { MessageState, Protocol } from "../types";
import type { BodyKind } from "./messageBodies";
import type { MessageProfile } from "./messageProfiles";

/**
 * The history's classes of message. `OK`, `ATTENTION`, `CLOSED`, `PENDING` and `RESENT` are
 * `routed` messages that differ in their deliveries; the others have none.
 */
export const K_OK = 0;
export const K_ATTENTION = 1;
export const K_CLOSED = 2;
export const K_PENDING = 3;
export const K_RECEIVED = 4;
export const K_RECEIVING = 5;
export const K_FILTERED = 6;
export const K_DUPLICATE = 7;
export const K_INVALID = 8;
export const K_ABORTED = 9;
export const K_OBSERVED = 10;
export const K_RESENT = 11;

export const KIND_STATE: readonly MessageState[] = [
  "routed",
  "routed",
  "routed",
  "routed",
  "received",
  "receiving",
  "filtered",
  "duplicate",
  "invalid",
  "aborted",
  "observed",
  "routed",
];

/** The states in alphabetical order, which is how the store sorts the state column. */
const STATE_ORDER = [...new Set(KIND_STATE)].sort();
export const stateRank = (kind: number): number =>
  STATE_ORDER.indexOf(KIND_STATE[kind] ?? "routed");

export const PREFIX: Record<Protocol, string> = {
  hl7_mllp: "MC",
  astm: "AS",
  au: "AU",
  delimited_file: "ROW-",
  raw: "RW",
};

/** The class the roll of message `i` asks for, as permille thresholds of the history. */
export const ROLL: readonly [number, number][] = [
  [790, K_OK],
  [840, K_ATTENTION],
  [865, K_CLOSED],
  [900, K_FILTERED],
  [930, K_DUPLICATE],
  [942, K_RESENT],
  [964, K_INVALID],
  [974, K_ABORTED],
];

export const isMultiFrame = (protocol: Protocol) => protocol === "astm" || protocol === "au";

/** Messages whose bytes were not (or not yet) a whole parsed message have no identifiers. */
export const isUnparsed = (kind: number) =>
  kind === K_INVALID || kind === K_ABORTED || kind === K_RECEIVING;

/** A message type as the store records it: HL7 types in full, the others by protocol. */
export function displayType(protocol: Protocol, type: string): string | null {
  switch (protocol) {
    case "hl7_mllp":
      return type;
    case "astm":
      return "astm";
    case "au":
      return "au";
    case "delimited_file":
      return "delimited";
    default:
      return null;
  }
}

export function kindFor(profile: MessageProfile, type: string): BodyKind {
  if (profile.protocol === "hl7_mllp") {
    if (type.startsWith("ADT")) return "hl7_adt";
    return type.startsWith("ORM") ? "hl7_orm" : "hl7_oru";
  }
  if (profile.protocol === "astm") return "astm_result";
  if (profile.protocol === "au") return "au_result";
  return profile.panel === "order" ? "csv_order" : "csv_row";
}

/** Why a message was filtered, invalid or aborted; `h` picks among the reasons. */
export function reasonFor(kind: number, protocol: Protocol, h: number) {
  const none = { reasonCode: null, abortReason: null };
  switch (kind) {
    case K_FILTERED:
      return {
        ...none,
        reasonCode: ["source_filter", "source_filter", "destination_filters", "no_destination"][
          h % 4
        ] as string,
      };
    case K_INVALID:
      return {
        ...none,
        reasonCode: ["parse_error", "missing_required_field"][h % 2] as string,
      };
    case K_ABORTED: {
      const reasons =
        protocol === "au"
          ? ["receive_timeout", "connection_lost", "incomplete_record"]
          : ["receive_timeout", "retry_limit_exceeded", "connection_lost", "empty_transfer"];
      return { ...none, abortReason: reasons[h % reasons.length] as string };
    }
    default:
      return none;
  }
}
