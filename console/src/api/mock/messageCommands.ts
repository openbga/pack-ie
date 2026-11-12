import { clone } from "@/lib/clone";
import type {
  ConfigDocument,
  Delivery,
  EngineError,
  ResendOutcome,
  ResendRequest,
  ResendResult,
} from "../types";
import { CLOSABLE, cancel, createdDelivery, requeue, settle } from "./deliveries";
import { type CreatedMessage, type Entry, type Fail, MessageHistory } from "./messageHistory";
import type { DestinationInfo } from "./messageProfiles";
import { Revisions } from "./messageRevisions";
import { judgeResend } from "./resendRules";
import { iso } from "./util";

const RESEND_SETTLE_MS = 2000;
const ROUTE_MS = 800;
const REASON_MAX = 500;

/** The operator's commands on the message history: resends, closing deliveries, late revisions. */
export class MessageCommands extends MessageHistory {
  protected readonly revisions: Revisions;
  /** Counts every change of a message or its deliveries, so cached orders of the grid go stale. */
  protected version = 0;
  private readonly timers = new Set<ReturnType<typeof setTimeout>>();

  constructor(
    seed: number,
    count: number,
    document: ConfigDocument,
    private readonly now: () => number,
    fail: Fail,
  ) {
    super(seed, count, document, now, fail);
    this.revisions = new Revisions(this.universe);
  }

  dispose(): void {
    for (const timer of this.timers) clearTimeout(timer);
    this.timers.clear();
  }

  private later(delayMs: number, run: () => void): void {
    const timer = setTimeout(() => {
      this.timers.delete(timer);
      run();
    }, delayMs);
    this.timers.add(timer);
  }

  protected idOf(i: number): string | null {
    return i < 0 ? null : `msg-${i}`;
  }

  private store(entry: Entry, deliveries: Delivery[]): void {
    if (entry.created === null) this.overrides.set(entry.i, deliveries);
    else entry.created.deliveries = deliveries;
    this.version += 1;
  }

  /** The reason of a resend, trimmed, or null when none was given; 1 to 500 characters. */
  private resendReason(given: string | undefined): string | null {
    const reason = given?.trim() ?? null;
    if (reason !== null && (reason === "" || [...reason].length > REASON_MAX)) {
      this.fail("validation", "A resend reason, when given, has 1 to 500 characters.");
    }
    return reason;
  }

  /**
   * The engine refuses to resend a message a newer revision supersedes, naming the newer one,
   * unless the operator confirms (compliance P10): the whole request is rejected.
   */
  private refuseSuperseded(entry: Entry): void {
    const newer = entry.created === null ? this.revisions.revisedBy(entry.i) : -1;
    if (newer < 0) return;
    const received = iso(this.universe.timeAt(newer));
    try {
      this.fail(
        "conflict",
        `A newer revision of this message exists (msg-${newer}, received ${received}). Resending this older version may reissue a corrected result; confirm that it is intended to resend it anyway.`,
      );
    } catch (error) {
      throw { ...(error as EngineError), rejection: "superseded" };
    }
  }

  /** A newer revision of the message arrives (journeys); the id of the revision, or null. */
  reviseNow(id: string): string | null {
    const entry = this.entry(id);
    const newer = entry.created === null ? this.revisions.reviseNow(entry.i) : -1;
    return this.idOf(newer);
  }

  resend(request: ResendRequest): ResendResult {
    const entry = this.entry(request.messageId);
    const reason = this.resendReason(request.reason);
    if (request.confirmSuperseded !== true) this.refuseSuperseded(entry);
    const profile = this.profileOf(entry);
    const state = this.summaryOfEntry(entry).state;
    const deliveries = clone(this.deliveriesOfEntry(entry));
    const wanted =
      request.destinations === "all"
        ? profile.destinations.filter((d) => d.enabled).map((d) => d.destinationId)
        : [...new Set(request.destinations)];
    const outcomes: ResendOutcome[] = [];
    const accepted: DestinationInfo[] = [];
    for (const id of wanted) {
      const destination = profile.destinations.find((d) => d.destinationId === id);
      const existing = deliveries.find((d) => d.destinationId === id);
      const rejected = judgeResend(request, state, existing, destination);
      outcomes.push({
        destinationId: id,
        destinationLabel: destination?.label ?? id,
        deliveryId: null,
        rejected,
      });
      if (rejected === null && destination !== undefined) accepted.push(destination);
    }
    if (request.mode === "new_message") {
      return { newMessageId: this.createMessage(entry, accepted, reason), destinations: outcomes };
    }
    const at = this.now();
    for (const outcome of outcomes) {
      const delivery = deliveries.find((d) => d.destinationId === outcome.destinationId);
      if (outcome.rejected !== null || delivery === undefined) continue;
      requeue(delivery, at, reason);
      outcome.deliveryId = delivery.id;
    }
    if (accepted.length > 0) {
      this.store(entry, deliveries);
      this.later(RESEND_SETTLE_MS, () => this.settleAll(entry, accepted));
    }
    return { newMessageId: null, destinations: outcomes };
  }

  /** The destinations answer the deliveries that were queued again. */
  private settleAll(entry: Entry, destinations: DestinationInfo[]): void {
    const deliveries = clone(this.deliveriesOfEntry(entry));
    const at = this.now();
    for (const delivery of deliveries) {
      const destination = destinations.find((d) => d.destinationId === delivery.destinationId);
      if (delivery.state !== "queued" || destination === undefined) continue;
      settle(delivery, destination, at, this.profileOf(entry).configVersion);
    }
    this.store(entry, deliveries);
  }

  /** `new_message`: a copy with `origin` resend that the pipeline routes shortly after. */
  private createMessage(
    source: Entry,
    destinations: DestinationInfo[],
    reason: string | null,
  ): string | null {
    if (destinations.length === 0) return null;
    const sourceId = source.created === null ? `msg-${source.i}` : source.created.id;
    const created: CreatedMessage = {
      id: `msg-new-${this.created.length + 1}`,
      source: source.i,
      resentFrom: sourceId,
      receivedAtMs: this.now(),
      routed: false,
      deliveries: [],
    };
    this.created.push(created);
    this.version += 1;
    const entry: Entry = { created, i: source.i };
    this.later(ROUTE_MS, () => {
      const profile = this.profileOf(entry);
      const number = this.created.indexOf(created) + 1;
      created.deliveries = destinations.map((destination) =>
        createdDelivery(
          created.id,
          `dlv-new-${number}`,
          profile.channelId,
          destination,
          profile.destinations.indexOf(destination),
          this.now(),
          profile.configVersion,
          reason,
        ),
      );
      created.routed = true;
      this.version += 1;
    });
    this.later(RESEND_SETTLE_MS, () => {
      const at = this.now();
      for (const delivery of created.deliveries) {
        const destination = destinations.find((d) => d.destinationId === delivery.destinationId);
        if (destination !== undefined) {
          settle(delivery, destination, at, this.profileOf(entry).configVersion);
        }
      }
      this.version += 1;
    });
    return created.id;
  }

  /** The entry holding a delivery, found from its id. */
  private entryOfDelivery(deliveryId: string): Entry | null {
    const made = /^dlv-new-(\d+)-\d+$/.exec(deliveryId);
    if (made) {
      const created = this.created[Number(made[1]) - 1];
      return created === undefined ? null : { created, i: created.source };
    }
    const match = /^dlv-(\d+)-\d+$/.exec(deliveryId);
    const i = match ? Number(match[1]) : -1;
    return i >= 0 && i < this.universe.count ? { created: null, i } : null;
  }

  closeDelivery(deliveryId: string, reason: string): Delivery {
    const text = reason.trim();
    if (text === "" || [...text].length > REASON_MAX) {
      this.fail("conflict", "Provide a reason of 1-500 characters.");
    }
    const entry = this.entryOfDelivery(deliveryId);
    const deliveries = entry === null ? [] : clone(this.deliveriesOfEntry(entry));
    const delivery = deliveries.find((d) => d.id === deliveryId);
    if (entry === null || delivery === undefined) this.fail("not_found", "Delivery not found.");
    if (delivery.state === "sending") {
      this.fail("conflict", "A delivery that is sending cannot be closed.");
    }
    if (!CLOSABLE.has(delivery.state)) this.fail("conflict", "This delivery is already resolved.");
    cancel(delivery, text, this.now());
    this.store(entry, deliveries);
    return clone(delivery);
  }
}
