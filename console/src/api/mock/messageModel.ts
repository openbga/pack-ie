import type { MessageDirection, MessageOrigin, MessageSummary, SourceFile } from "../types";
import {
  type BodyInput,
  type BodyKind,
  buildAborted,
  buildInvalid,
  buildOriginal,
  targetSize,
} from "./messageBodies";
import {
  displayType,
  isMultiFrame,
  isUnparsed,
  K_ABORTED,
  K_ATTENTION,
  K_CLOSED,
  K_DUPLICATE,
  K_FILTERED,
  K_INVALID,
  K_OBSERVED,
  K_OK,
  K_PENDING,
  K_RECEIVED,
  K_RECEIVING,
  K_RESENT,
  KIND_STATE,
  kindFor,
  PREFIX,
  ROLL,
  reasonFor,
} from "./messageKinds";
import type { MessageProfile } from "./messageProfiles";
import { PANELS } from "./panels";
import { hash32 } from "./prng";
import { DAY, iso, pad } from "./util";

const MAX_COUNT = 4_000_000;
const SLOT_TABLE = 1024;
/** Messages this many positions older were received under one configuration version older. */
const MESSAGES_PER_VERSION = 90_000;

/** What the grid shows of a message before its deliveries are added. */
export type BaseSummary = Omit<MessageSummary, "deliverySummary" | "deliveries">;

/**
 * The retained message history, generated on demand: message index `i` (0 is the newest) maps
 * to every field through hashes of `(seed, i)`; nothing is stored per message. A duplicate or
 * a message resent from an earlier one carries that message's content (`contentOf`).
 */
export class MessageUniverse {
  readonly count: number;
  readonly gapMs: number;
  /** Alphabetical rank of each channel label, for sorting. */
  readonly labelRank: number[];
  readonly typeNames: string[];
  private readonly slotTable = new Uint8Array(SLOT_TABLE);
  private readonly typeRank: number[][];

  constructor(
    readonly seed: number,
    count: number,
    readonly baseMs: number,
    readonly profiles: MessageProfile[],
  ) {
    this.count = profiles.length === 0 ? 0 : Math.min(count, MAX_COUNT);
    this.gapMs = Math.min(60_000, Math.max(1000, Math.floor((30 * DAY) / Math.max(this.count, 1))));
    this.fillSlots();
    const labels = [...profiles].map((p) => p.label).sort();
    this.labelRank = profiles.map((p) => labels.indexOf(p.label));
    const typesOf = (p: MessageProfile) =>
      p.types.map((type) => displayType(p.protocol, type) ?? "");
    this.typeNames = [...new Set(profiles.flatMap(typesOf))].sort();
    this.typeRank = profiles.map((p) => typesOf(p).map((t) => this.typeNames.indexOf(t) + 1));
  }

  private fillSlots(): void {
    const total = this.profiles.reduce((sum, p) => sum + p.weight, 0);
    let cursor = 0;
    let acc = 0;
    this.profiles.forEach((profile, slot) => {
      acc += profile.weight;
      const end = Math.round((acc / total) * SLOT_TABLE);
      while (cursor < end && cursor < SLOT_TABLE) this.slotTable[cursor++] = slot;
    });
    while (cursor < SLOT_TABLE) this.slotTable[cursor++] = Math.max(this.profiles.length - 1, 0);
  }

  timeAt(i: number): number {
    return this.baseMs - i * this.gapMs - (hash32(this.seed, i, 10) % Math.floor(this.gapMs * 0.6));
  }

  slotOf(i: number): number {
    return this.slotTable[hash32(this.seed, i, 12) % SLOT_TABLE] ?? 0;
  }

  private profileAt(slot: number): MessageProfile {
    return this.profiles[slot] as MessageProfile;
  }

  /** The class the roll of message `i` asks for, before its channel is considered. */
  private rolled(i: number): number {
    const u = hash32(this.seed, i, 11) % 1000;
    return ROLL.find(([limit]) => u < limit)?.[1] ?? K_OK;
  }

  /** Whether message `j` can be the original of a duplicate or a resend. */
  private isOriginal(j: number, allowed: readonly number[]): boolean {
    if (j < 0 || j >= this.count) return false;
    return allowed.includes(this.rolled(j)) && !this.profileAt(this.slotOf(j)).tap;
  }

  /** The older message a duplicate repeats, or -1. */
  duplicateOf(i: number): number {
    const original = i + 1 + (hash32(this.seed, i, 16) % 7);
    return this.isOriginal(original, [K_OK, K_ATTENTION, K_CLOSED, K_FILTERED]) ? original : -1;
  }

  /** The older message a resend was made from, or -1. */
  resentFrom(i: number): number {
    const original = i + 1 + (hash32(this.seed, i, 21) % 40);
    return this.isOriginal(original, [K_OK, K_ATTENTION, K_CLOSED]) ? original : -1;
  }

  /** The class of message `i`, before any operator action. */
  kindOf(i: number): number {
    const own = this.profileAt(this.slotOf(i));
    if (own.tap) return K_OBSERVED;
    // The newest message is still arriving: a multi-frame transfer, or a message being routed.
    if (i === 0) return isMultiFrame(own.protocol) ? K_RECEIVING : K_RECEIVED;
    if (i < 4) return K_PENDING;
    const rolled = this.rolled(i);
    switch (rolled) {
      case K_DUPLICATE:
        return this.duplicateOf(i) < 0 ? K_OK : K_DUPLICATE;
      case K_RESENT:
        return this.resentFrom(i) < 0 ? K_OK : K_RESENT;
      case K_ABORTED:
        return isMultiFrame(own.protocol) ? rolled : K_OK;
      default:
        return rolled;
    }
  }

  /** The message whose content message `i` carries: itself, or the original it repeats. */
  contentOf(i: number, kind: number = this.kindOf(i)): number {
    if (kind === K_DUPLICATE) return this.duplicateOf(i);
    return kind === K_RESENT ? this.resentFrom(i) : i;
  }

  /** The profile of the channel message `i` belongs to. */
  profileOf(i: number): MessageProfile {
    return this.profileAt(this.slotOf(this.contentOf(i)));
  }

  directionOf(i: number, kind: number = this.kindOf(i)): MessageDirection {
    if (kind !== K_OBSERVED) return "inbound";
    return hash32(this.seed, i, 23) % 2 === 0 ? "a_to_b" : "b_to_a";
  }

  typeIndex(content: number, slot: number): number {
    const types = this.profileAt(slot).types.length;
    return hash32(this.seed, content, 17) % Math.max(types, 1);
  }

  /** The type as the grid shows it; null for a message that did not parse. */
  typeOf(i: number, kind: number = this.kindOf(i)): string | null {
    if (isUnparsed(kind)) return null;
    const content = this.contentOf(i, kind);
    const slot = this.slotOf(content);
    const profile = this.profileAt(slot);
    return displayType(profile.protocol, profile.types[this.typeIndex(content, slot)] ?? "");
  }

  /** Sort key of the type: 0 for none, then alphabetical. */
  typeRankOf(i: number, kind: number = this.kindOf(i)): number {
    if (isUnparsed(kind)) return 0;
    const content = this.contentOf(i, kind);
    const slot = this.slotOf(content);
    return this.typeRank[slot]?.[this.typeIndex(content, slot)] ?? 0;
  }

  private bodyKind(content: number, slot: number): BodyKind {
    const profile = this.profileAt(slot);
    return kindFor(profile, profile.types[this.typeIndex(content, slot)] ?? "");
  }

  private resultCount(content: number, slot: number, kind: BodyKind): number {
    if (kind === "csv_row" || kind === "hl7_adt" || kind === "csv_order") return 1;
    const profile = this.profileAt(slot);
    const max = kind === "hl7_orm" ? 4 : Math.min(PANELS[profile.panel].tests.length, 6);
    return 1 + (hash32(this.seed, content, 14) % max);
  }

  /** Size in bytes of the original of message `i`; equals the length of `originalText`. */
  sizeOf(i: number, kind: number = this.kindOf(i)): number {
    if (isUnparsed(kind)) return this.originalText(i, kind).length;
    const content = this.contentOf(i, kind);
    const slot = this.slotOf(content);
    const body = this.bodyKind(content, slot);
    return targetSize(body, this.resultCount(content, slot, body), hash32(this.seed, content, 19));
  }

  controlNumber(content: number): number {
    return 10_000_000 + (hash32(this.seed, content, 15) % 90_000_000);
  }

  private hasControl(content: number, slot: number): boolean {
    const protocol = this.profileAt(slot).protocol;
    if (protocol === "au") return false;
    return protocol === "hl7_mllp" || hash32(this.seed, content, 18) % 10 !== 0;
  }

  controlId(content: number, slot: number): string | null {
    if (!this.hasControl(content, slot)) return null;
    return `${PREFIX[this.profileAt(slot).protocol]}${this.controlNumber(content)}`;
  }

  private sampleNumber(content: number): number {
    return 9000 + Math.floor((this.count - 1 - content) / 3);
  }

  sampleId(content: number): string {
    return `SMP-${pad(this.sampleNumber(content), 7)}`;
  }

  private patientNumber(content: number): number {
    return hash32(this.seed, Math.floor(content / 3), 3) % 250_000;
  }

  patientId(content: number): string {
    return `PT-${pad(this.patientNumber(content), 6)}`;
  }

  /** The three identifiers of message `i`; all null when its bytes did not parse. */
  identifiersOf(i: number, kind: number = this.kindOf(i)) {
    if (isUnparsed(kind)) return { controlId: null, sampleId: null, patientId: null };
    const content = this.contentOf(i, kind);
    const slot = this.slotOf(content);
    return {
      controlId: this.controlId(content, slot),
      sampleId: this.sampleId(content),
      patientId: this.patientId(content),
    };
  }

  private accession(content: number): string {
    return `ACC-26-${pad(this.sampleNumber(content), 6)}`;
  }

  /** Everything the body generators need for the content of message `content`. */
  bodyInput(content: number): BodyInput {
    const slot = this.slotOf(content);
    const profile = this.profileAt(slot);
    const kind = this.bodyKind(content, slot);
    const n = this.resultCount(content, slot, kind);
    return {
      kind,
      panel: profile.panel,
      seed: this.seed,
      index: content,
      n,
      sender: profile.sender,
      control: this.controlId(content, slot) ?? "",
      sample: this.sampleId(content),
      patientNum: this.patientNumber(content),
      patient: this.patientId(content),
      accession: this.accession(content),
      atMs: this.timeAt(content),
      target: targetSize(kind, n, hash32(this.seed, content, 19)),
    };
  }

  /** The original bytes of message `i`, as the source sent them. */
  originalText(i: number, kind: number = this.kindOf(i)): string {
    if (isUnparsed(kind)) {
      const slot = this.slotOf(i);
      const protocol = this.profileAt(slot).protocol;
      const input = { ...this.bodyInput(i), target: 0 };
      return kind === K_INVALID ? buildInvalid(input, protocol) : buildAborted(input, protocol);
    }
    return buildOriginal(this.bodyInput(this.contentOf(i, kind)));
  }

  /** Where message `i` came from and the code it was answered with, for the record. */
  reasonOf(i: number, kind: number = this.kindOf(i)) {
    return reasonFor(kind, this.profileOf(i).protocol, hash32(this.seed, i, 22));
  }

  /** The code the sender was answered with, and when; null for messages nobody answered. */
  responseOf(i: number, kind: number = this.kindOf(i)): { code: string; atMs: number } | null {
    const profile = this.profileOf(i);
    const answered = [K_OK, K_ATTENTION, K_CLOSED, K_PENDING, K_RECEIVED, K_FILTERED, K_DUPLICATE];
    if (kind === K_INVALID && profile.protocol === "hl7_mllp") {
      return { code: "AR", atMs: this.timeAt(i) + 12 };
    }
    if (!answered.includes(kind) || profile.protocol === "delimited_file") return null;
    const code = profile.protocol === "hl7_mllp" ? "AA" : "ACK";
    return { code, atMs: this.timeAt(i) + 8 + (hash32(this.seed, i, 26) % 30) };
  }

  /** A stand-in for the SHA-256 of message `i`'s original bytes: equal content, equal hash. */
  contentHash(i: number, kind: number = this.kindOf(i)): string {
    const content = this.contentOf(i, kind);
    return Array.from({ length: 8 }, (_, k) =>
      hash32(this.seed, content, 40 + k)
        .toString(16)
        .padStart(8, "0"),
    ).join("");
  }

  /** What duplicate detection compares, for the messages that have an identity. */
  sourceIdentityOf(i: number, kind: number = this.kindOf(i)): string | null {
    if (isUnparsed(kind) || kind === K_OBSERVED || kind === K_RESENT) return null;
    const content = this.contentOf(i, kind);
    const profile = this.profileAt(this.slotOf(content));
    const { controlId, sampleId } = this.identifiersOf(i, kind);
    switch (profile.protocol) {
      case "hl7_mllp":
        return `${profile.sender}|MAINLAB|${controlId ?? ""}`;
      case "astm":
        return `${profile.sender}|${iso(this.timeAt(content))}|${sampleId ?? ""}`;
      case "delimited_file":
        return this.fileOf(i, kind)?.name ?? null;
      default:
        return `${profile.sender}|${sampleId ?? ""}`;
    }
  }

  /** The file and rows of a message read from a file; null for the other protocols. */
  fileOf(i: number, kind: number = this.kindOf(i)): SourceFile | null {
    const content = this.contentOf(i, kind);
    const profile = this.profileAt(this.slotOf(content));
    if (profile.protocol !== "delimited_file" || kind === K_RESENT) return null;
    const day = new Date(this.timeAt(content)).toISOString().slice(0, 10).replace(/-/g, "");
    const row = 2 + (hash32(this.seed, content, 25) % 40);
    return {
      name: `results-${day}-${pad(Math.floor(content / 40) % 1000, 3)}.csv`,
      fileHash: this.contentHash(content, K_OK),
      firstRow: row,
      lastRow: row,
    };
  }

  /** The configuration version message `i` was handled under. */
  configVersionOf(i: number): number {
    return Math.max(1, this.profileOf(i).configVersion - Math.floor(i / MESSAGES_PER_VERSION));
  }

  /** The grid row of message `i` without its deliveries. */
  base(i: number, kind: number = this.kindOf(i)): BaseSummary {
    const content = this.contentOf(i, kind);
    const profile = this.profileOf(i);
    const direction = this.directionOf(i, kind);
    const original = kind === K_RESENT ? `msg-${content}` : null;
    return {
      id: `msg-${i}`,
      channelId: profile.channelId,
      channelLabel: profile.label,
      seq: this.count - i,
      direction,
      state: KIND_STATE[kind] ?? "routed",
      origin: this.originOf(kind),
      receivedAt: iso(this.timeAt(i)),
      protocol: profile.protocol,
      messageType: this.typeOf(i, kind),
      ...this.identifiersOf(i, kind),
      sizeBytes: this.sizeOf(i, kind),
      sourcePeer:
        kind === K_RESENT || profile.peers.length === 0
          ? null
          : (profile.peers[hash32(this.seed, i, 24) % profile.peers.length] ?? null),
      configVersion: this.configVersionOf(i),
      resentFrom: original,
    };
  }

  private originOf(kind: number): MessageOrigin {
    if (kind === K_RESENT) return "resend";
    return kind === K_OBSERVED ? "tap" : "received";
  }
}
