// A structural comparison of two configuration documents as the console models them, for the
// version history: what rolling back would change, as `field: old → new` in words. Generic over
// the document's fields, so a setting the editors gain shows up without code here; settings the
// console does not model are not in the document and do not appear.
import type { ConfigDocument } from "@/api/types";
import { sameJson } from "./configDocument";

/** One difference. `before` and `after` are words; `detail` expands an array summary. */
export interface FieldChange {
  label: string;
  before: string;
  after: string;
  detail?: FieldChange[];
}

export interface ItemChange {
  id: string;
  label: string;
  kind: "added" | "removed" | "changed";
  /** What differs, for a changed item. */
  fields: FieldChange[];
}

/** `old` is the active configuration and `new` the version, as a rollback would change it. */
export interface DocumentComparison {
  endpoints: ItemChange[];
  channels: ItemChange[];
  thresholds: FieldChange[];
}

const FIELD_LABELS: Record<string, string> = {
  id: "ID",
  endpointAId: "Endpoint A",
  endpointBId: "Endpoint B",
  endpointId: "Endpoint",
  pairingReceiver: "Receiver",
  bufferBytes: "Buffer per direction (bytes)",
  tapFramer: "Message framing",
  initialMs: "Initial backoff (ms)",
  maxMs: "Maximum backoff (ms)",
  maxAttempts: "Maximum attempts",
  baudRate: "Baud rate",
  idleTimeoutSeconds: "Idle timeout (s)",
  connectTimeoutMs: "Connect timeout (ms)",
  allowAnyPeer: "Allow any peer",
  bindAddress: "Bind address",
  pollIntervalSec: "Poll interval (s)",
  filePattern: "File pattern",
  remotePath: "Remote path",
  hostKeyFingerprint: "Host key fingerprint",
  secretName: "Secret name",
  passphraseSecretName: "Passphrase secret name",
  authMethod: "Authentication",
  ackMode: "Acknowledgement mode",
  astmRole: "Link role",
  templateJson: "Column template",
  duplicateDetection: "Detect duplicate messages",
  maxMessages: "Keep at most (messages)",
  days: "Keep for (days)",
  ignoreCase: "Ignore case",
  codeTables: "Code tables",
  endpointDownSeconds: "Endpoint down limit (s)",
  queueDepthWarning: "Delivery queue warning",
  bufferPressurePercent: "Buffer pressure limit (%)",
  diskFreeMinMib: "Free disk minimum (MiB)",
};

/** What an array holds, for "3 rules → 4 rules". */
const NOUNS = new Map([
  ["filters", "filter"],
  ["mappings", "rule"],
  ["rules", "rule"],
  ["then", "rule"],
  ["else", "rule"],
  ["parts", "part"],
  ["destinations", "destination"],
]);

/** Arrays whose changed members are shown inline, not summarised: destinations are records. */
const INLINE = new Set(["destinations"]);

/** Stands for the key of a value inside the code tables, whose own keys are data. */
const DATA = "(data)";

/** Keys that hold an endpoint's id; shown as the endpoint's name. */
const ENDPOINT_REFS = new Set(["endpointAId", "endpointBId", "endpointId"]);

type Json = unknown;
type Record_ = Record<string, Json>;

const isRecord = (value: Json): value is Record_ =>
  typeof value === "object" && value !== null && !Array.isArray(value);

function humanize(key: string): string {
  const words = key.replace(/([a-z0-9])([A-Z])/g, "$1 $2").toLowerCase();
  return words.charAt(0).toUpperCase() + words.slice(1);
}

interface Context {
  /** The endpoint's name for an id, from either document. */
  endpointName: (id: string) => string;
}

/** A value in words. */
function show(value: Json, key: string, context: Context): string {
  if (value === undefined || value === null) return "none";
  if (typeof value === "boolean") return value ? "yes" : "no";
  if (value === "") return "empty";
  if (typeof value === "string") {
    if (ENDPOINT_REFS.has(key)) return context.endpointName(value);
    return /^[a-z0-9]+(_[a-z0-9]+)+$/.test(value) ? value.replaceAll("_", " ") : value;
  }
  if (Array.isArray(value)) return value.length === 0 ? "none" : value.map(String).join(", ");
  if (isRecord(value)) {
    return Object.entries(value)
      .map(([name, member]) => `${name}: ${show(member, name, context)}`)
      .join(", ");
  }
  return String(value);
}

/** The first few settings of a record, for an added or removed array member. */
function describeMember(member: Json, context: Context): string {
  if (!isRecord(member)) return show(member, "", context);
  return Object.entries(member)
    .filter(
      ([key, value]) => key !== "id" && typeof value !== "object" && value !== "" && value !== null,
    )
    .slice(0, 4)
    .map(
      ([key, value]) =>
        `${(FIELD_LABELS[key] ?? humanize(key)).toLowerCase()} ${show(value, key, context)}`,
    )
    .join(", ");
}

const hasId = (member: Json): member is Record_ & { id: string } =>
  isRecord(member) && typeof member.id === "string";

function memberName(member: Json, index: number): string {
  if (isRecord(member)) {
    if (typeof member.label === "string" && member.label !== "") return member.label;
    if (typeof member.id === "string") return member.id;
  }
  return `#${index + 1}`;
}

const joinLabel = (parent: string, child: string) =>
  parent === "" ? child : `${parent} › ${child}`;

function diffMembers(
  label: string,
  key: string,
  before: Json[],
  after: Json[],
  context: Context,
): FieldChange[] {
  const noun = NOUNS.get(key) ?? "item";
  const Noun = noun.charAt(0).toUpperCase() + noun.slice(1);
  const byId = [...before, ...after].every(hasId);
  const pairs: { old?: Json; next?: Json; name: string }[] = byId
    ? [
        ...after.map((next, index) => ({
          next,
          old: before.find((each) => hasId(each) && hasId(next) && each.id === next.id),
          name: memberName(next, index),
        })),
        ...before
          .filter((old) => !after.some((next) => hasId(old) && hasId(next) && next.id === old.id))
          .map((old, index) => ({ old, name: memberName(old, index) })),
      ]
    : Array.from({ length: Math.max(before.length, after.length) }, (_, index) => ({
        old: before[index],
        next: after[index],
        name: `#${index + 1}`,
      }));
  const changes = pairs.flatMap(({ old, next, name }): FieldChange[] => {
    const at = `${Noun} ${name}`;
    if (old === undefined) {
      return [{ label: at, before: "not present", after: describeMember(next, context) }];
    }
    if (next === undefined) {
      return [{ label: at, before: describeMember(old, context), after: "removed" }];
    }
    return diffValue(at, key, old, next, context);
  });
  if (INLINE.has(key))
    return changes.map((change) => ({ ...change, label: joinLabel(label, change.label) }));
  const count = (list: Json[]) => `${list.length} ${noun}${list.length === 1 ? "" : "s"}`;
  const summary = before.length === after.length ? `${count(after)}, changed` : count(after);
  return [{ label, before: count(before), after: summary, detail: changes }];
}

function diffArray(label: string, key: string, before: Json[], after: Json[], context: Context) {
  if (
    before.every((each) => typeof each !== "object") &&
    after.every((each) => typeof each !== "object")
  ) {
    return [{ label, before: show(before, key, context), after: show(after, key, context) }];
  }
  return diffMembers(label, key, before, after, context);
}

function diffRecord(
  label: string,
  parentKey: string,
  before: Record_,
  after: Record_,
  context: Context,
): FieldChange[] {
  const keys = [...new Set([...Object.keys(before), ...Object.keys(after)])];
  // Table and code names are data, not fields: they keep their own spelling.
  const isData = parentKey === "codeTables" || parentKey === DATA;
  return keys.flatMap((key) => {
    const name = isData ? key : (FIELD_LABELS[key] ?? humanize(key));
    const child = isData ? DATA : key;
    // The members of an inline array are named on their own, not under the array.
    const at = INLINE.has(key) ? label : joinLabel(label, name);
    return diffValue(at, child, before[key], after[key], context);
  });
}

function diffValue(
  label: string,
  key: string,
  before: Json,
  after: Json,
  context: Context,
): FieldChange[] {
  if (sameJson(before, after)) return [];
  if (isRecord(before) && isRecord(after)) return diffRecord(label, key, before, after, context);
  if (Array.isArray(before) && Array.isArray(after))
    return diffArray(label, key, before, after, context);
  return [{ label, before: show(before, key, context), after: show(after, key, context) }];
}

function compareItems<Item extends { id: string; label: string }>(
  active: readonly Item[],
  version: readonly Item[],
  context: Context,
): ItemChange[] {
  const changes: ItemChange[] = [];
  for (const item of version) {
    const old = active.find((each) => each.id === item.id);
    if (old === undefined) {
      changes.push({ id: item.id, label: item.label, kind: "added", fields: [] });
    } else {
      const fields = diffValue("", "", old, item, context);
      if (fields.length > 0)
        changes.push({ id: item.id, label: item.label, kind: "changed", fields });
    }
  }
  for (const old of active) {
    if (!version.some((each) => each.id === old.id)) {
      changes.push({ id: old.id, label: old.label, kind: "removed", fields: [] });
    }
  }
  return changes;
}

/** How rolling back from `active` to `version` would change the configuration. */
export function compareDocuments(
  active: ConfigDocument,
  version: ConfigDocument,
): DocumentComparison {
  const names = new Map(
    [...active.endpoints, ...version.endpoints].map((each) => [each.id, each.label]),
  );
  const context: Context = { endpointName: (id) => names.get(id) ?? id };
  return {
    endpoints: compareItems(active.endpoints, version.endpoints, context),
    channels: compareItems(active.channels, version.channels, context),
    thresholds: diffValue("", "", active.alertThresholds, version.alertThresholds, context),
  };
}

/** Whether the comparison holds any difference. */
export function hasDifferences(comparison: DocumentComparison): boolean {
  return (
    comparison.endpoints.length > 0 ||
    comparison.channels.length > 0 ||
    comparison.thresholds.length > 0
  );
}
