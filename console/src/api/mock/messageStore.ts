import { clone } from "@/lib/clone";
import type {
  BodyRequest,
  ConfigDocument,
  Delivery,
  MessageBody,
  MessageDetail,
  MessageFields,
  MessageFilter,
  MessagePage,
  MessageQuery,
  MessageSummary,
} from "../types";
import { buildTransformed } from "./messageBodies";
import { MessageCommands } from "./messageCommands";
import { parsedFields } from "./messageFields";
import type { Entry, Fail } from "./messageHistory";
import { compareSummaries, idPrefixOf, MessageIndex, matchesSummary } from "./messageIndex";
import { K_DUPLICATE, K_INVALID } from "./messageKinds";
import { renderBody } from "./messageRender";
import { iso } from "./util";

const PAGE_LIMIT = 1000;
const MIN_TEXT = 3;

/** The message history with its queries and the operator's commands on it. */
export class MessageStore extends MessageCommands {
  private readonly index: MessageIndex;

  constructor(
    seed: number,
    count: number,
    document: ConfigDocument,
    now: () => number,
    fail: Fail,
  ) {
    super(seed, count, document, now, fail);
    this.index = new MessageIndex(this.universe, {
      deliveryStatesAt: (i, kind) => this.statesOf(i, kind),
    });
  }

  // ── Queries ─────────────────────────────────────────────────────────────────────────

  list(query: MessageQuery): MessagePage {
    const { offset, limit } = this.checked(query);
    return this.page(this.index.run(query, this.version), query, offset, limit);
  }

  /**
   * The same page as `list`, searched in steps with the page's own work in between: what the
   * console asks for, where the engine would do this in another process.
   */
  async listInSteps(query: MessageQuery): Promise<MessagePage> {
    const { offset, limit } = this.checked(query);
    const order = await this.index.runInSteps(query, () => this.version);
    return this.page(order, query, offset, limit);
  }

  /** Refuses a query the engine would, and gives its offset and limit as whole numbers. */
  private checked(query: MessageQuery): { offset: number; limit: number } {
    const text = query.filter.text?.trim() ?? "";
    if (text !== "" && [...text].length < MIN_TEXT) {
      this.fail("validation", `A text search needs at least ${MIN_TEXT} characters.`);
    }
    const idText = query.filter.messageIdPrefix?.trim() ?? "";
    const idPrefix = idText === "" ? null : idPrefixOf(idText);
    if (idPrefix !== null && "error" in idPrefix) this.fail("validation", idPrefix.error);
    const offset = Math.max(0, Math.floor(query.offset));
    const limit = Math.floor(query.limit);
    if (limit < 1 || limit > PAGE_LIMIT) {
      this.fail("validation", `The page limit must be from 1 to ${PAGE_LIMIT}.`);
    }
    return { offset, limit };
  }

  private page(order: Int32Array, query: MessageQuery, offset: number, limit: number): MessagePage {
    const createdRows = this.created
      .map((created) => this.summaryOfCreated(created))
      .filter((row) => matchesSummary(row, query.filter, () => this.originalText(row.id)))
      .sort((a, b) => compareSummaries(a, b, query.sort));
    const total = order.length + createdRows.length;
    return { rows: this.window(order, createdRows, query, offset, limit), total, offset, limit };
  }

  /**
   * The messages `filter` selects, oldest first, at most `limit` of them, and how many match in
   * all: what an export reads, page by page, as the engine does.
   */
  select(filter: MessageFilter, limit: number): { rows: MessageSummary[]; total: number } {
    const rows: MessageSummary[] = [];
    let total = 0;
    do {
      const page = this.list({
        filter,
        sort: { field: "receivedAt", descending: false },
        offset: rows.length,
        limit: Math.min(PAGE_LIMIT, limit - rows.length),
      });
      total = page.total;
      rows.push(...page.rows);
      if (page.rows.length === 0) break;
    } while (rows.length < Math.min(total, limit));
    return { rows, total };
  }

  /** The rows from `offset`, the created messages merged into the generated ones in sort order. */
  private window(
    order: Int32Array,
    createdRows: MessageSummary[],
    query: MessageQuery,
    offset: number,
    limit: number,
  ): MessageSummary[] {
    const rows: MessageSummary[] = [];
    let u = 0;
    let c = 0;
    let current: MessageSummary | null = null;
    const stop = Math.min(order.length + createdRows.length, offset + limit);
    let position = 0;
    // While created messages are left to place, merge by comparing them with the generated ones.
    for (; position < stop && c < createdRows.length; position++) {
      if (current === null && u < order.length) current = this.summaryOf(order[u] as number);
      const made = createdRows[c];
      const takeCreated =
        made !== undefined &&
        (current === null || compareSummaries(made, current, query.sort) <= 0);
      const row = takeCreated ? made : current;
      if (takeCreated) c++;
      else {
        u++;
        current = null;
      }
      if (position >= offset && row) rows.push(row);
    }
    // Only generated messages remain: build the summaries of the requested window alone, never
    // those of the rows before it (the last page of 500,000 messages must not cost 500,000).
    const first = u + Math.max(0, offset - position);
    const last = u + (stop - position);
    for (let k = first; k < last; k++) rows.push(this.summaryOf(order[k] as number));
    return rows;
  }

  /** The original bytes of a message, as text. */
  originalText(id: string): string {
    const entry = this.entry(id);
    return this.universe.originalText(entry.i);
  }

  get(id: string): MessageDetail {
    const entry = this.entry(id);
    const u = this.universe;
    const summary = this.summaryOfEntry(entry);
    const { i, created } = entry;
    const kind = u.kindOf(i);
    const profile = this.profileOf(entry);
    const response = created === null ? u.responseOf(i, kind) : null;
    return {
      summary,
      committedAt: summary.state === "receiving" ? null : iso(Date.parse(summary.receivedAt) + 5),
      contentHash: u.contentHash(i, kind),
      sourceIdentity: created === null ? u.sourceIdentityOf(i, kind) : null,
      sourceEndpointId: profile.sourceEndpointId,
      sourceEndpointLabel: profile.sourceEndpointLabel,
      sourceFile: created === null ? u.fileOf(i, kind) : null,
      responseCode: response?.code ?? null,
      respondedAt: response === null ? null : iso(response.atMs),
      ...(created === null ? u.reasonOf(i, kind) : { reasonCode: null, abortReason: null }),
      duplicateOf: created === null && kind === K_DUPLICATE ? `msg-${u.duplicateOf(i)}` : null,
      revisionOf: created === null ? this.idOf(this.revisions.revisionOf(i)) : null,
      revisedBy: created === null ? this.idOf(this.revisions.revisedBy(i)) : null,
      deliveries: clone(this.deliveriesOfEntry(entry)),
    };
  }

  /** What the engine lists from the original: nothing parses in an invalid message. */
  fields(id: string): MessageFields {
    const entry = this.entry(id);
    const kind = this.universe.kindOf(entry.i);
    return parsedFields(
      this.universe.originalText(entry.i),
      this.profileOf(entry).protocol,
      kind !== K_INVALID || entry.created !== null,
    );
  }

  /** The bytes of the transformed body of `delivery`: the mapped output, or the original as is. */
  private transformedText(entry: Entry, delivery: Delivery): string {
    const u = this.universe;
    const profile = this.profileOf(entry);
    const destination = profile.destinations.find(
      (d) => d.destinationId === delivery.destinationId,
    );
    const content = u.contentOf(entry.i);
    if (destination === undefined || !destination.hasMappings) return u.originalText(entry.i);
    return buildTransformed(u.bodyInput(content), destination.protocol);
  }

  body(request: BodyRequest): MessageBody {
    const entry = this.entry(request.messageId);
    const u = this.universe;
    const { which } = request;
    let text: string;
    let hash: string;
    if (which.kind === "original") {
      text = u.originalText(entry.i);
      hash = u.contentHash(entry.i);
    } else {
      const delivery = this.deliveriesOfEntry(entry).find((d) => d.id === which.deliveryId);
      if (delivery === undefined) this.fail("not_found", "Delivery not found.");
      if (delivery.transformedHash === null) {
        this.fail("not_found", "This delivery has no transformed body.");
      }
      text = this.transformedText(entry, delivery);
      hash = delivery.transformedHash;
    }
    const { content, truncated } = renderBody(text, request.as);
    return {
      messageId: request.messageId,
      which,
      as: request.as,
      content,
      byteLength: text.length,
      hash,
      truncated,
    };
  }
}
