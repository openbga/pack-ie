import { K_ATTENTION, K_CLOSED, K_OK } from "./messageKinds";
import type { MessageUniverse } from "./messageModel";

/** A revision chain is looked for before every `STRIDE`th message from `FIRST` on (index 0 is the newest). */
const FIRST = 40;
const STRIDE = 120;
/** A chain is within the `SPAN` messages before its group's mark. */
const SPAN = 100;
const MAX_CHAIN = 3;
const FIRST_ROUTED_AFTER = 4;

const ELIGIBLE = [K_OK, K_ATTENTION, K_CLOSED];

/**
 * The revision chains of the generated history: a corrected result is a newer message of the
 * same channel that revises the older one. Chains of two or three messages are laid over the
 * history at fixed places (one group of `SPAN` messages in every `STRIDE`), so the same
 * messages are superseded in every run and the history itself is unchanged: a revision shares
 * its channel but not its identifiers. `reviseNow` adds one when a journey needs a revision to arrive late.
 *
 * Indexes are history positions: 0 is the newest message, so a revision has a lower one.
 */
export class Revisions {
  private readonly chains = new Map<number, readonly number[]>();
  /** Revisions made by `reviseNow`: older message to newer, and back. */
  private readonly lateNewer = new Map<number, number>();
  private readonly lateOlder = new Map<number, number>();

  constructor(private readonly universe: MessageUniverse) {}

  private canBeRevised(i: number): boolean {
    return (
      i >= FIRST_ROUTED_AFTER &&
      i < this.universe.count &&
      ELIGIBLE.includes(this.universe.kindOf(i))
    );
  }

  private isEligible(i: number, slot: number): boolean {
    return this.canBeRevised(i) && this.universe.slotOf(i) === slot;
  }

  /**
   * The chain of the group `k`, oldest first, taken from the messages before `FIRST + k * STRIDE`:
   * the first that can be revised, then the next ones of its channel; empty when it has none.
   */
  private chain(k: number): readonly number[] {
    const known = this.chains.get(k);
    if (known !== undefined) return known;
    const from = FIRST + k * STRIDE;
    const found: number[] = [];
    let slot = -1;
    for (let i = from; i >= Math.max(FIRST_ROUTED_AFTER, from - SPAN); i--) {
      if (found.length === MAX_CHAIN) break;
      if (found.length === 0 ? this.canBeRevised(i) : this.isEligible(i, slot)) {
        found.push(i);
        slot = this.universe.slotOf(i);
      }
    }
    const chain = found.length < 2 ? [] : found;
    this.chains.set(k, chain);
    return chain;
  }

  private chainOf(i: number): readonly number[] {
    return this.chain(Math.max(0, Math.ceil((i - FIRST) / STRIDE)));
  }

  /** The message that message `i` revises, the one before it in its chain; -1 when none. */
  revisionOf(i: number): number {
    const chain = this.chainOf(i);
    const at = chain.indexOf(i);
    return at > 0 ? (chain[at - 1] as number) : (this.lateOlder.get(i) ?? -1);
  }

  /** The newest message that revises message `i`: it is superseded by it; -1 when none. */
  revisedBy(i: number): number {
    const chain = this.chainOf(i);
    const at = chain.indexOf(i);
    let newest = at >= 0 && at < chain.length - 1 ? (chain.at(-1) as number) : -1;
    if (newest < 0) newest = this.lateNewer.get(i) ?? -1;
    // A revision that arrived later may itself have been revised since.
    for (let later = this.lateNewer.get(newest); later !== undefined; ) {
      newest = later;
      later = this.lateNewer.get(newest);
    }
    return newest;
  }

  /**
   * Message `i` is revised by the next message of its channel that can be: the journey of a
   * revision arriving while an operator looks at the message. Returns that message, or -1 when
   * `i` cannot be revised, is superseded already or no such message exists.
   */
  reviseNow(i: number): number {
    if (!this.canBeRevised(i) || this.revisedBy(i) >= 0) return -1;
    const slot = this.universe.slotOf(i);
    for (let j = i - 1; j >= FIRST_ROUTED_AFTER; j--) {
      if (!this.isEligible(j, slot) || this.revisionOf(j) >= 0 || this.revisedBy(j) >= 0) continue;
      this.lateNewer.set(i, j);
      this.lateOlder.set(j, i);
      return j;
    }
    return -1;
  }
}
