/** Finders that fail a test when a console document lacks the item, and the diff of two documents. */

import type {
  BridgeChannelConfig,
  ConfigDocument,
  EndpointConfig,
  MessageChannelConfig,
} from "../../types";
import { isEqual, isRecord } from "../util";
import { IDS } from "./configFixtures";

/** The leaf paths at which two JSON values differ (lists of plain values as a whole). */
export function diffPaths(a: unknown, b: unknown, prefix = ""): string[] {
  if (isEqual(a, b)) return [];
  if (Array.isArray(a) && Array.isArray(b) && a.length === b.length && a.every(isRecord)) {
    return a.flatMap((item, i) => diffPaths(item, b[i], `${prefix}[${i}]`));
  }
  if (isRecord(a) && isRecord(b)) {
    const keys = [...new Set([...Object.keys(a), ...Object.keys(b)])];
    return keys.flatMap((key) =>
      diffPaths(a[key], b[key], prefix === "" ? key : `${prefix}.${key}`),
    );
  }
  return [prefix];
}

export function endpoint<K extends EndpointConfig["kind"]>(
  doc: ConfigDocument,
  id: string,
  kind: K,
): Extract<EndpointConfig, { kind: K }> {
  const found = doc.endpoints.find((e) => e.id === id);
  if (found?.kind !== kind) throw new Error(`no ${kind} endpoint ${id}`);
  return found as Extract<EndpointConfig, { kind: K }>;
}

export function bridge(doc: ConfigDocument): BridgeChannelConfig {
  const found = doc.channels.find((c) => c.id === IDS.bridge);
  if (found?.kind !== "bridge") throw new Error("no bridge");
  return found;
}

export function message(doc: ConfigDocument, id: string = IDS.message): MessageChannelConfig {
  const found = doc.channels.find((c) => c.id === id);
  if (found?.kind !== "message") throw new Error("no message channel");
  return found;
}

export function destination(doc: ConfigDocument, index: number) {
  const found = message(doc).destinations[index];
  if (found === undefined) throw new Error(`no destination ${index}`);
  return found;
}
