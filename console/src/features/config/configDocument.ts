// Pure helpers over the runtime configuration document: no engine calls, no React.
import type { ChannelConfig, ConfigDocument, EndpointConfig } from "@/api/types";

/** A lower-case id suggestion for a label: `Chem analyzer #2` becomes `chem-analyzer-2`. */
export function slugify(text: string): string {
  return text
    .normalize("NFKD")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 48)
    .replace(/-+$/, "");
}

/** `slugify`, made unique among `taken` with a numeric suffix; empty when the text has no letters. */
export function uniqueSlug(text: string, taken: ReadonlySet<string>): string {
  const base = slugify(text);
  if (base === "" || !taken.has(base)) return base;
  for (let n = 2; ; n += 1) {
    const candidate = `${base}-${n}`;
    if (!taken.has(candidate)) return candidate;
  }
}

/** The first of `prefix1`, `prefix2`, … that is not in `taken`: ids for new rules. */
export function nextId(prefix: string, taken: Iterable<string>): string {
  const used = new Set(taken);
  for (let n = 1; ; n += 1) {
    const candidate = `${prefix}${n}`;
    if (!used.has(candidate)) return candidate;
  }
}

/** Structural equality of two JSON values, ignoring key order. */
export function sameJson(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (Array.isArray(a) || Array.isArray(b)) {
    return (
      Array.isArray(a) &&
      Array.isArray(b) &&
      a.length === b.length &&
      a.every((item, index) => sameJson(item, b[index]))
    );
  }
  if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) return false;
  const left = a as Record<string, unknown>;
  const right = b as Record<string, unknown>;
  const keys = new Set([...Object.keys(left), ...Object.keys(right)]);
  for (const key of keys) {
    // An absent key and an undefined value mean the same in JSON.
    if (!sameJson(left[key], right[key])) return false;
  }
  return true;
}

/** One channel that uses an endpoint, and in which role. */
export interface EndpointReference {
  channelId: string;
  channelLabel: string;
  role: string;
}

function channelReferences(channel: ChannelConfig, endpointId: string): EndpointReference[] {
  const base = { channelId: channel.id, channelLabel: channel.label };
  const roles: string[] = [];
  if (channel.kind === "bridge") {
    if (channel.endpointAId === endpointId) roles.push("Endpoint A");
    if (channel.endpointBId === endpointId) roles.push("Endpoint B");
  } else {
    if (channel.source.endpointId === endpointId) roles.push("Source");
    for (const [index, destination] of channel.destinations.entries()) {
      if (destination.endpointId === endpointId) roles.push(`Destination ${index + 1}`);
    }
  }
  return roles.map((role) => ({ ...base, role }));
}

/** The channels that use `endpointId`, so an endpoint in use is never deleted from under them. */
export function endpointReferences(doc: ConfigDocument, endpointId: string): EndpointReference[] {
  return doc.channels.flatMap((channel) => channelReferences(channel, endpointId));
}

/** `items` with the entry whose id is `originalId` replaced by `next`, or `next` appended. */
function upsertById<Item extends { id: string }>(
  items: readonly Item[],
  originalId: string | undefined,
  next: Item,
): Item[] {
  if (originalId === undefined) return [...items, next];
  return items.map((item) => (item.id === originalId ? next : item));
}

export function withEndpoint(
  doc: ConfigDocument,
  originalId: string | undefined,
  next: EndpointConfig,
): ConfigDocument {
  return { ...doc, endpoints: upsertById(doc.endpoints, originalId, next) };
}

export function withChannel(
  doc: ConfigDocument,
  originalId: string | undefined,
  next: ChannelConfig,
): ConfigDocument {
  return { ...doc, channels: upsertById(doc.channels, originalId, next) };
}

export function withoutEndpoint(doc: ConfigDocument, id: string): ConfigDocument {
  return { ...doc, endpoints: doc.endpoints.filter((endpoint) => endpoint.id !== id) };
}

export function withoutChannel(doc: ConfigDocument, id: string): ConfigDocument {
  return { ...doc, channels: doc.channels.filter((channel) => channel.id !== id) };
}

/** A destination that `after` no longer has although its channel is still there. */
export interface RemovedDestination {
  channelId: string;
  channelLabel: string;
  destinationLabel: string;
}

/** The destinations removed from channels that remain; their unresolved deliveries are closed. */
export function removedDestinations(
  before: ConfigDocument,
  after: ConfigDocument,
): RemovedDestination[] {
  return before.channels.flatMap((old) => {
    const next = after.channels.find((each) => each.id === old.id);
    if (old.kind !== "message" || next?.kind !== "message") return [];
    return old.destinations
      .filter((destination) => !next.destinations.some((each) => each.id === destination.id))
      .map((destination) => ({
        channelId: old.id,
        channelLabel: old.label,
        destinationLabel: destination.label,
      }));
  });
}
