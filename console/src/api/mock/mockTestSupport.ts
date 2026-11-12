import { afterEach, vi } from "vitest";
import type {
  ChannelConfig,
  ChannelSnapshot,
  ConfigDocument,
  ConfigIssue,
  Delivery,
  DestinationConfig,
  EndpointConfig,
  EngineError,
  MessageChannelConfig,
  MessageDetail,
  MessageFilter,
  MessageQuery,
  ResendResult,
} from "../types";
import { MockEngineApi, type MockOptions } from "./index";

/**
 * Registers the clean-up for the calling test file and returns a factory for mocks with
 * instant responses; `defaults` are the file's usual options, `options` override per call.
 */
export function mockApiFactory(defaults: MockOptions = { messageCount: 500, seed: 7 }) {
  const apis: MockEngineApi[] = [];
  afterEach(() => {
    for (const api of apis.splice(0)) api.dispose();
    vi.useRealTimers();
  });
  return (options: MockOptions = {}): MockEngineApi => {
    const api = new MockEngineApi({ latency: "none", ...defaults, ...options });
    apis.push(api);
    return api;
  };
}

/** The error a call rejects with; fails when it does not reject. */
export async function rejection(promise: Promise<unknown>): Promise<EngineError> {
  try {
    await promise;
  } catch (error) {
    return error as EngineError;
  }
  throw new Error("expected the call to reject");
}

export async function channelOf(api: MockEngineApi, id: string): Promise<ChannelSnapshot> {
  const found = (await api.getSnapshot()).channels.find((c) => c.id === id);
  if (found === undefined) throw new Error(`no channel ${id}`);
  return found;
}

export function messageChannel(document: ConfigDocument, id: string): MessageChannelConfig {
  const channel = document.channels.find((c) => c.id === id);
  if (channel?.kind !== "message") throw new Error(`no message channel ${id}`);
  return channel;
}

/** A newest-first message query. */
export function query(
  filter: MessageFilter = {},
  overrides: Partial<MessageQuery> = {},
): MessageQuery {
  return {
    filter,
    sort: { field: "receivedAt", descending: true },
    offset: 0,
    limit: 50,
    ...overrides,
  };
}

export async function rowsOf(api: MockEngineApi, filter: MessageFilter, limit = 1000) {
  return (await api.listMessages(query(filter, { limit }))).rows;
}

/** The original bytes of a message as the console shows them. */
export async function originalText(
  api: MockEngineApi,
  id: string,
  as: "text" | "hex" = "text",
): Promise<string> {
  return (await api.getMessageBody({ messageId: id, which: { kind: "original" }, as })).content;
}

/** The first message whose deliveries include one in `state` (and satisfying `pick`), with it. */
export async function withDelivery(
  api: MockEngineApi,
  state: Delivery["state"],
  filter: MessageFilter = {},
  pick: (delivery: Delivery) => boolean = () => true,
): Promise<{ detail: MessageDetail; delivery: Delivery }> {
  for (const row of await rowsOf(api, filter)) {
    if (!row.deliveries.some((chip) => chip.state === state)) continue;
    const detail = await api.getMessage(row.id);
    const delivery = detail.deliveries.find((d) => d.state === state && pick(d));
    if (delivery !== undefined) return { detail, delivery };
  }
  throw new Error(`no message with a ${state} delivery`);
}

/** Resends a message as a new message to all its destinations, the duplicate risk confirmed. */
export function resendAsNew(api: MockEngineApi, messageId: string): Promise<ResendResult> {
  return api.resendMessage({
    messageId,
    destinations: "all",
    mode: "new_message",
    confirmDuplicateRisk: true,
  });
}

// Configuration documents: the validation and config tests change one rule at a time.

/** The issues the engine finds in `doc`. */
export async function check(doc: ConfigDocument, api: MockEngineApi): Promise<ConfigIssue[]> {
  return (await api.validateConfig(doc)).issues;
}

/** The paths of the issues of one severity. */
export function at(issues: ConfigIssue[], severity: ConfigIssue["severity"]): string[] {
  return issues.filter((i) => i.severity === severity).map((i) => i.path);
}

export function endpointIndex(doc: ConfigDocument, match: (e: EndpointConfig) => boolean): number {
  const index = doc.endpoints.findIndex(match);
  if (index < 0) throw new Error("no such endpoint");
  return index;
}

/** The index of a message channel; the XN-1000 channel unless `id` says otherwise. */
export function messageIndex(doc: ConfigDocument, id = "ch-msg-xn1000"): number {
  const index = doc.channels.findIndex((c) => c.id === id);
  if (index < 0) throw new Error("no such channel");
  return index;
}

export function messageChannelAt(doc: ConfigDocument, index: number): MessageChannelConfig {
  const channel = doc.channels[index];
  if (channel?.kind !== "message") throw new Error("expected a message channel");
  return channel;
}

export function patchEndpoint(
  doc: ConfigDocument,
  index: number,
  patch: Record<string, unknown>,
): void {
  doc.endpoints[index] = { ...doc.endpoints[index], ...patch } as EndpointConfig;
}

export function patchChannel(
  doc: ConfigDocument,
  index: number,
  patch: Record<string, unknown>,
): void {
  doc.channels[index] = { ...doc.channels[index], ...patch } as ChannelConfig;
}

export function destinationOf(channel: MessageChannelConfig, at = 0): DestinationConfig {
  const destination = channel.destinations[at];
  if (destination === undefined) throw new Error("expected a destination");
  return destination;
}

/** Changes destination `at` of the channel at `index`. */
export function patchDestination(
  doc: ConfigDocument,
  index: number,
  patch: Partial<DestinationConfig>,
  at = 0,
): void {
  const channel = messageChannelAt(doc, index);
  const destinations = channel.destinations.map((each, i) =>
    i === at ? { ...each, ...patch } : each,
  );
  patchChannel(doc, index, { destinations });
}
