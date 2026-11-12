import type {
  ConfigDocument,
  Delivery,
  DeliveryState,
  ErrorCode,
  FieldError,
  MessageSummary,
} from "../types";
import { buildDeliveries, chipOf, deliveryStates, summarize } from "./deliveries";
import { type DeliveryContext, planOf } from "./deliveryContext";
import { MessageUniverse } from "./messageModel";
import { buildProfiles, type MessageProfile } from "./messageProfiles";
import { iso } from "./util";

export type Fail = (code: ErrorCode, message: string, fieldErrors?: FieldError[]) => never;

/** A message the operator created by resending as a new message. */
export interface CreatedMessage {
  id: string;
  /** The universe message whose original bytes it carries. */
  source: number;
  resentFrom: string;
  receivedAtMs: number;
  /** The pipeline has finished and created the deliveries. */
  routed: boolean;
  deliveries: Delivery[];
}

/** One message of the store: a generated one, or one created by a resend. */
export type Entry = { created: null; i: number } | { created: CreatedMessage; i: number };

/**
 * The generated history with the messages and deliveries operator actions changed: what a
 * message, its deliveries and its grid row are, whichever way it came to be.
 */
export class MessageHistory {
  readonly universe: MessageUniverse;
  protected readonly overrides = new Map<number, Delivery[]>();
  protected readonly created: CreatedMessage[] = [];

  constructor(
    seed: number,
    count: number,
    document: ConfigDocument,
    now: () => number,
    protected readonly fail: Fail,
  ) {
    this.universe = new MessageUniverse(seed, count, now(), buildProfiles(document));
  }

  get count(): number {
    return this.universe.count + this.created.length;
  }

  // ── Deliveries of generated messages ────────────────────────────────────────────────

  private contextOf(i: number, kind: number): DeliveryContext | null {
    const plan = planOf(kind);
    if (plan === null) return null;
    const u = this.universe;
    const content = u.contentOf(i, kind);
    const profile = u.profileOf(i);
    return {
      profile,
      plan,
      seed: u.seed,
      index: i,
      idPrefix: `dlv-${i}`,
      receivedAtMs: u.timeAt(i),
      control: u.controlId(content, u.slotOf(content)) ?? "",
      configVersion: u.configVersionOf(i),
    };
  }

  protected statesOf(i: number, kind: number): DeliveryState[] {
    const stored = this.overrides.get(i);
    if (stored !== undefined) return stored.map((delivery) => delivery.state);
    const context = this.contextOf(i, kind);
    return context === null ? [] : deliveryStates(context);
  }

  private deliveriesOf(i: number, kind: number): Delivery[] {
    const stored = this.overrides.get(i);
    if (stored !== undefined) return stored;
    const context = this.contextOf(i, kind);
    return context === null ? [] : buildDeliveries(context);
  }

  // ── Entries ─────────────────────────────────────────────────────────────────────────

  protected entry(id: string): Entry {
    const made = /^msg-new-(\d+)$/.exec(id);
    const created = made ? this.created[Number(made[1]) - 1] : undefined;
    if (created !== undefined) return { created, i: created.source };
    const match = /^msg-(\d+)$/.exec(id);
    const i = match ? Number(match[1]) : -1;
    if (i < 0 || i >= this.universe.count) {
      return this.fail("not_found", "The message was not found. It may be past its retention.");
    }
    return { created: null, i };
  }

  protected profileOf(entry: Entry): MessageProfile {
    return this.universe.profileOf(entry.i);
  }

  protected deliveriesOfEntry(entry: Entry): Delivery[] {
    return entry.created === null
      ? this.deliveriesOf(entry.i, this.universe.kindOf(entry.i))
      : entry.created.deliveries;
  }

  protected summaryOf(i: number): MessageSummary {
    const kind = this.universe.kindOf(i);
    const deliveries = this.deliveriesOf(i, kind);
    return {
      ...this.universe.base(i, kind),
      deliverySummary: summarize(deliveries.map((d) => d.state)),
      deliveries: deliveries.map(chipOf),
    };
  }

  protected summaryOfCreated(created: CreatedMessage): MessageSummary {
    const u = this.universe;
    const base = u.base(created.source);
    const index = this.created.indexOf(created) + 1;
    return {
      ...base,
      id: created.id,
      seq: u.count + index,
      state: created.routed ? "routed" : "received",
      origin: "resend",
      receivedAt: iso(created.receivedAtMs),
      sourcePeer: null,
      configVersion: u.profileOf(created.source).configVersion,
      resentFrom: created.resentFrom,
      deliverySummary: summarize(created.deliveries.map((d) => d.state)),
      deliveries: created.deliveries.map(chipOf),
    };
  }

  protected summaryOfEntry(entry: Entry): MessageSummary {
    return entry.created === null ? this.summaryOf(entry.i) : this.summaryOfCreated(entry.created);
  }
}
