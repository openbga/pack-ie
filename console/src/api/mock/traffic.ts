import type { Unsubscribe } from "../engine";
import type { Protocol, TapFramer, TrafficSegment } from "../types";
import {
  ACK,
  type BodyInput,
  type BodyKind,
  buildOriginal,
  EB,
  ENQ,
  EOT,
  NAK,
  SB,
} from "./messageBodies";
import type { PanelKey } from "./panels";
import type { Rng } from "./prng";
import type { TrafficDelta } from "./runtimeState";
import { compactTime, iso, pad, toBase64 } from "./util";

export type TrafficStyle = "astm" | "hl7" | "au" | "raw";

interface Piece {
  /** The pane: bridges A to B or B to A; message channels the Source or Destinations pane. */
  direction: TrafficSegment["direction"];
  bytes: string;
  /** This piece completes a framed message. */
  ends?: boolean;
  /** Message channels: relative to the engine. */
  flow?: NonNullable<TrafficSegment["flow"]>;
  /** Message channels: what the piece is (received, response, sent, reply). */
  role?: NonNullable<TrafficSegment["role"]>;
  endpointId?: string;
  destinationId?: string;
}

/** A destination of a message channel as the tap sees it. */
interface TrafficDestination {
  id: string;
  endpointId: string;
  style: TrafficStyle;
}

/** What a message channel's tap needs to tell its two panes apart. */
interface MessageTap {
  sourceEndpointId: string;
  /** Enabled destinations only: a disabled one is sent nothing. */
  destinations: TrafficDestination[];
}

export function styleOf(framer: TapFramer): TrafficStyle {
  if (framer === "none") return "raw";
  return framer === "hl7_mllp" ? "hl7" : framer;
}

export function styleOfProtocol(protocol: Protocol): TrafficStyle {
  if (protocol === "astm" || protocol === "au") return protocol;
  return protocol === "hl7_mllp" ? "hl7" : "raw";
}

function syntheticInput(kind: BodyKind, panel: PanelKey, rng: Rng, atMs: number): BodyInput {
  const n = rng.int(1, 600_000);
  return {
    kind,
    panel,
    seed: 11,
    index: n,
    n: rng.int(1, 4),
    sender: "ANALYZER",
    control: `MC${pad(10_000_000 + n, 8)}`,
    sample: `SMP-${pad(n, 7)}`,
    patientNum: n % 250_000,
    patient: `PT-${pad(n % 250_000, 6)}`,
    accession: `ACC-26-${pad(n % 1_000_000, 6)}`,
    atMs,
    target: 0,
  };
}

function astmTransaction(panel: PanelKey, rng: Rng, atMs: number): Piece[] {
  const body = buildOriginal(syntheticInput("astm_result", panel, rng, atMs));
  const frames = body.split(/(?<=\n)/).filter((frame) => frame !== "");
  const pieces: Piece[] = [
    { direction: "a_to_b", bytes: ENQ },
    { direction: "b_to_a", bytes: ACK },
  ];
  for (const frame of frames) {
    pieces.push({ direction: "a_to_b", bytes: frame }, { direction: "b_to_a", bytes: ACK });
  }
  pieces.push({ direction: "a_to_b", bytes: EOT, ends: true });
  return pieces;
}

function hl7Transaction(panel: PanelKey, rng: Rng, atMs: number): Piece[] {
  const input = syntheticInput("hl7_oru", panel, rng, atMs);
  const ack = `MSH|^~\\&|LIS|HOSP|ANALYZER|MAINLAB|${compactTime(atMs)}||ACK|A${input.control}|P|2.5.1\rMSA|AA|${input.control}\r`;
  return [
    { direction: "a_to_b", bytes: `${SB}${buildOriginal(input)}${EB}\r`, ends: true },
    { direction: "b_to_a", bytes: `${SB}${ack}${EB}\r`, ends: true },
  ];
}

function auTransaction(panel: PanelKey, rng: Rng, atMs: number): Piece[] {
  return [
    {
      direction: "a_to_b",
      bytes: buildOriginal(syntheticInput("au_result", panel, rng, atMs)),
      ends: true,
    },
    { direction: "b_to_a", bytes: ACK },
  ];
}

function rawTransaction(panel: PanelKey, rng: Rng, atMs: number): Piece[] {
  const row = buildOriginal(syntheticInput("csv_row", panel, rng, atMs)).split("\r\n")[1] ?? "";
  const pieces: Piece[] = [{ direction: "a_to_b", bytes: `${row}\r\n`, ends: true }];
  if (rng.chance(0.5)) pieces.push({ direction: "b_to_a", bytes: "OK\r\n" });
  return pieces;
}

const BUILDERS: Record<TrafficStyle, (panel: PanelKey, rng: Rng, atMs: number) => Piece[]> = {
  astm: astmTransaction,
  hl7: hl7Transaction,
  au: auTransaction,
  raw: rawTransaction,
};

/**
 * A message channel's transaction. The Source pane (`a_to_b`) holds what the source sent
 * (`received`) and the engine's ACK or response to it (`response`). The Destinations pane
 * (`b_to_a`) holds, for each destination, what the engine sent (`sent`) and what that
 * destination answered (`reply`, now and then a NAK), each piece tagged with the destination.
 */
function messageTransaction(
  style: TrafficStyle,
  tap: MessageTap,
  panel: PanelKey,
  rng: Rng,
  atMs: number,
): Piece[] {
  const pieces: Piece[] = BUILDERS[style](panel, rng, atMs).map((piece) => ({
    direction: "a_to_b",
    bytes: piece.bytes,
    flow: piece.direction === "a_to_b" ? "inbound" : "outbound",
    role: piece.direction === "a_to_b" ? "received" : "response",
    endpointId: tap.sourceEndpointId,
    ends: piece.direction === "a_to_b" && piece.ends === true,
  }));
  for (const destination of tap.destinations) {
    for (const piece of BUILDERS[destination.style](panel, rng, atMs)) {
      const reply = piece.direction === "b_to_a";
      pieces.push({
        direction: "b_to_a",
        bytes: reply && piece.bytes === ACK && rng.chance(0.08) ? NAK : piece.bytes,
        flow: reply ? "inbound" : "outbound",
        role: reply ? "reply" : "sent",
        endpointId: destination.endpointId,
        destinationId: destination.id,
        ends: !reply && piece.ends === true,
      });
    }
  }
  return pieces;
}

export interface TrafficOptions {
  style: TrafficStyle;
  /** Message channels: the source endpoint and the destinations; null for a bridge. */
  message: MessageTap | null;
  panel: PanelKey;
  rng: Rng;
  now: () => number;
  /** False while the channel moves no bytes (stopped, retrying, overflowing). */
  isLive: () => boolean;
  /** What each batch adds to the channel's counters. */
  onTraffic: (delta: TrafficDelta) => void;
  onSegments: (segments: TrafficSegment[]) => void;
}

/**
 * What a batch adds to the channel's counters. A bridge counts each pane; a message channel
 * counts what the source sent in and what the engine sent to destinations, not the replies.
 */
function deltaOf(pieces: Piece[], isMessage: boolean): TrafficDelta {
  const delta: TrafficDelta = { bytesIn: 0, bytesOut: 0, messagesIn: 0, messagesOut: 0 };
  for (const piece of pieces) {
    const message = piece.ends === true ? 1 : 0;
    const toSource = piece.direction === "a_to_b";
    const counted = toSource && (!isMessage || piece.flow === "inbound");
    const sent = !toSource && (!isMessage || piece.flow === "outbound");
    if (counted) {
      delta.bytesIn += piece.bytes.length;
      delta.messagesIn += message;
    } else if (sent) {
      delta.bytesOut += piece.bytes.length;
      delta.messagesOut += message;
    }
  }
  return delta;
}

/**
 * Starts a synthetic tap: 1 to 3 segments every 600-900 ms until the returned function is
 * called. Nothing is emitted while the channel is not live.
 */
export function startTraffic(options: TrafficOptions): Unsubscribe {
  const { style, message, panel, rng, now } = options;
  const queue: Piece[] = [];
  let seq = 1;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let active = true;

  const nextPiece = (): Piece | undefined => {
    if (queue.length === 0) {
      const atMs = now();
      queue.push(
        ...(message === null
          ? BUILDERS[style](panel, rng, atMs)
          : messageTransaction(style, message, panel, rng, atMs)),
      );
    }
    return queue.shift();
  };

  const emit = (): void => {
    if (!active) return;
    if (options.isLive()) {
      const pieces: Piece[] = [];
      const count = rng.int(1, 3);
      for (let k = 0; k < count; k++) {
        const piece = nextPiece();
        if (piece !== undefined) pieces.push(piece);
      }
      const at = iso(now());
      options.onTraffic(deltaOf(pieces, message !== null));
      options.onSegments(
        pieces.map((piece) => ({
          seq: seq++,
          at,
          direction: piece.direction,
          base64: toBase64(piece.bytes),
          ...(piece.flow === undefined
            ? {}
            : {
                flow: piece.flow,
                role: piece.role,
                endpointId: piece.endpointId,
                destinationId: piece.destinationId ?? null,
              }),
        })),
      );
    }
    timer = setTimeout(emit, rng.int(600, 900));
  };

  timer = setTimeout(emit, rng.int(600, 900));
  return () => {
    active = false;
    if (timer !== null) clearTimeout(timer);
    timer = null;
  };
}
