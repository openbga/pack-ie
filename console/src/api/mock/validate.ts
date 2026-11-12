import type {
  ChannelConfig,
  ConfigDocument,
  ConfigIssue,
  DestinationConfig,
  MessageChannelConfig,
  RetentionPolicy,
  RetryPolicy,
} from "../types";
import { checkEndpointRef, validateConnector } from "./validateConnector";
import { validateEndpoints } from "./validateEndpoints";
import { blank, checkPositive, checkRange, error, type Issues, required } from "./validateIssues";
import { validateFilters, validateMappings } from "./validateRules";

function validateThresholds(document: ConfigDocument, issues: Issues): void {
  const t = document.alertThresholds;
  const at = "alertThresholds";
  checkRange(
    issues,
    `${at}.endpointDownSeconds`,
    t.endpointDownSeconds,
    { min: 1, max: 86_400 },
    "The endpoint down limit",
  );
  checkRange(
    issues,
    `${at}.queueDepthWarning`,
    t.queueDepthWarning,
    { min: 0, max: 10_000_000 },
    "The delivery queue limit",
  );
  checkRange(
    issues,
    `${at}.bufferPressurePercent`,
    t.bufferPressurePercent,
    { min: 1, max: 100 },
    "The buffer pressure limit",
  );
  checkRange(
    issues,
    `${at}.diskFreeMinMib`,
    t.diskFreeMinMib,
    { min: 1, max: 10_000_000 },
    "The free disk limit",
  );
}

function validateRetry(
  retry: RetryPolicy,
  path: string,
  issues: Issues,
  forever: "allowed" | "not allowed",
): void {
  checkPositive(
    issues,
    `${path}.initialMs`,
    retry.initialMs,
    "The first retry delay",
    "retry_invalid",
  );
  if (retry.maxMs < retry.initialMs) {
    error(
      issues,
      `${path}.maxMs`,
      "The longest retry delay must not be shorter than the first.",
      "retry_invalid",
    );
  }
  if (retry.maxAttempts === null) {
    if (forever === "not allowed") {
      error(issues, `${path}.maxAttempts`, "Maximum attempts is required.", "retry_invalid");
    }
  } else if (!Number.isInteger(retry.maxAttempts) || retry.maxAttempts < 1) {
    error(
      issues,
      `${path}.maxAttempts`,
      "Attempts must be a whole number, 1 or more.",
      "retry_invalid",
    );
  }
}

function checkPairing(
  document: ConfigDocument,
  channel: Extract<ChannelConfig, { kind: "bridge" }>,
  path: string,
  issues: Issues,
): void {
  const receiverNeeded = channel.pairing === "receiver_then_sender";
  if (receiverNeeded && channel.pairingReceiver === null) {
    error(issues, `${path}.pairingReceiver`, "Choose which side receives first (A or B).");
  } else if (!receiverNeeded && channel.pairingReceiver !== null) {
    error(
      issues,
      `${path}.pairingReceiver`,
      "A receiver is only used with the receiver-then-sender strategy.",
    );
  }
  if (channel.pairing !== "wait_for_listener_then_pair") return;
  const kinds = [channel.endpointAId, channel.endpointBId].map(
    (id) => document.endpoints.find((e) => e.id === id)?.kind,
  );
  if (!kinds.includes("tcp_listener")) {
    error(
      issues,
      `${path}.pairing`,
      "This strategy needs one side to be a TCP listener.",
      "listener_strategy",
    );
  }
}

function validateBridge(
  document: ConfigDocument,
  channel: Extract<ChannelConfig, { kind: "bridge" }>,
  path: string,
  issues: Issues,
): void {
  checkEndpointRef(document, channel, channel.endpointAId, `${path}.endpointAId`, issues);
  checkEndpointRef(document, channel, channel.endpointBId, `${path}.endpointBId`, issues);
  if (!blank(channel.endpointAId) && channel.endpointAId === channel.endpointBId) {
    error(
      issues,
      `${path}.endpointBId`,
      "Side B must be a different endpoint than side A.",
      "same_endpoint",
    );
  }
  checkPairing(document, channel, path, issues);
  checkPositive(issues, `${path}.bufferBytes`, channel.bufferBytes, "The buffer size");
  validateRetry(channel.retry, `${path}.retry`, issues, "allowed");
  validateTapRetention(channel.tapRetention, `${path}.tapRetention`, issues);
}

/** A tap keeps what it records for 1 to 36,500 days and at most 1,000 to 1,000,000,000 messages. */
function validateTapRetention(retention: RetentionPolicy, path: string, issues: Issues): void {
  const { days, maxMessages } = retention;
  if (days !== null && (days < 1 || days > 36_500)) {
    error(issues, `${path}.days`, "Keep recorded messages for 1 to 36,500 days.");
  }
  if (maxMessages !== null && (maxMessages < 1000 || maxMessages > 1_000_000_000)) {
    error(issues, `${path}.maxMessages`, "Keep between 1,000 and 1,000,000,000 messages.");
  }
}

function validateDestination(
  document: ConfigDocument,
  channel: MessageChannelConfig,
  destination: DestinationConfig,
  path: string,
  issues: Issues,
): void {
  validateConnector(document, channel, destination, "destination", path, issues);
  const label = destination.label.trim();
  if (label === "" || label.length > 100) {
    error(issues, `${path}.label`, "The label is required and at most 100 characters.");
  }
  // `passthrough_protocol`: with no mapping the original message is delivered as it is.
  if (destination.mappings.length === 0 && destination.protocol !== channel.source.protocol) {
    error(
      issues,
      `${path}.protocol`,
      "This destination needs a mapping: its protocol differs from the source's.",
      "passthrough_protocol",
    );
  }
  validateFilters(destination.filters, path, issues);
  validateMappings(destination, path, issues);
  validateRetry(destination.retry, `${path}.retry`, issues, "not allowed");
}

function validateMessageChannel(
  document: ConfigDocument,
  channel: MessageChannelConfig,
  path: string,
  issues: Issues,
): void {
  validateConnector(document, channel, channel.source, "source", `${path}.source`, issues);
  if (channel.destinations.length === 0)
    error(issues, `${path}.destinations`, "Add at least one destination.");
  const connectorIds = new Set<string>([channel.source.id]);
  channel.destinations.forEach((destination, index) => {
    const at = `${path}.destinations[${index}]`;
    validateDestination(document, channel, destination, at, issues);
    if (connectorIds.has(destination.id))
      error(issues, `${at}.id`, `Duplicate connector id "${destination.id}".`);
    connectorIds.add(destination.id);
  });
  validateFilters(channel.filters, path, issues);
  const { days, maxMessages } = channel.retention;
  if (days !== null && days < 1)
    error(issues, `${path}.retention.days`, "Keep messages for at least 1 day.");
  if (maxMessages !== null && maxMessages < 1) {
    error(issues, `${path}.retention.maxMessages`, "Keep at least 1 message.");
  }
}

function validateChannels(document: ConfigDocument, issues: Issues): void {
  const ids = new Set<string>();
  document.channels.forEach((channel, index) => {
    const path = `channels[${index}]`;
    if (blank(channel.id)) error(issues, `${path}.id`, "An id is required.");
    else if (ids.has(channel.id))
      error(issues, `${path}.id`, `Duplicate channel id "${channel.id}".`, "duplicate_id");
    ids.add(channel.id);
    required(issues, `${path}.label`, channel.label, "The label");
    if (channel.kind === "bridge") validateBridge(document, channel, path, issues);
    else validateMessageChannel(document, channel, path, issues);
  });
}

/** Checks a whole document the way the engine does: errors block apply, warnings do not. */
export function validateDocument(
  document: ConfigDocument,
  secrets: ReadonlySet<string>,
): ConfigIssue[] {
  const issues: Issues = [];
  if (!Number.isInteger(document.version))
    error(issues, "version", "The version must be a whole number.");
  validateThresholds(document, issues);
  validateEndpoints(document, secrets, issues);
  validateChannels(document, issues);
  return issues;
}
