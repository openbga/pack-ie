import { type ReactNode, useState } from "react";
import { Button } from "@/components/Button";
import { ChecklistPopover } from "@/components/ChecklistPopover";
import { DateTimeField } from "@/components/DateTimeField";
import { FindField } from "@/components/FindField";
import { Select } from "@/components/Select";
import { Toolbar } from "@/components/Toolbar";
import { without, withValue } from "@/lib/optionalKeys";
import { useLatest } from "@/lib/useLatest";
import {
  DEFAULT_FILTERS,
  isFiltered,
  isMessageIdPrefix,
  isSearchableText,
  type MessageFilters,
  MIN_TEXT,
  RELATIVE_MS,
  resolveTime,
} from "./messageParams";
import {
  DELIVERY_STATES,
  deliveryLabel,
  directionText,
  MESSAGE_DIRECTIONS,
  MESSAGE_STATES,
  messageStateStatus,
} from "./messageWording";
import { QuickFilters } from "./QuickFilters";

interface ChannelOption {
  id: string;
  label: string;
}

interface MessageFilterBarProps {
  value: MessageFilters;
  /** The whole next set of filters, whenever one changes. */
  onChange: (next: MessageFilters) => void;
  /** The channels to choose from. */
  channels: readonly ChannelOption[];
  /** Controls that follow the filters, such as refresh and the column chooser. */
  actions?: ReactNode;
}

const ANY = "__any";

const RANGES = [
  { id: "-1h", label: "Last hour" },
  { id: "-24h", label: "Last 24 hours" },
  { id: "-7d", label: "Last 7 days" },
  { id: "-30d", label: "Last 30 days" },
  { id: "any", label: "Any time" },
  { id: "custom", label: "Custom range…" },
];

const MATCHES = [
  { id: "prefix", label: "Starts with" },
  { id: "exact", label: "Exactly" },
];

/** What the time range control shows: a quick range, any time, or explicit times. */
function rangeOf(value: MessageFilters, custom: boolean): string {
  if (custom) return "custom";
  if (value.from === undefined && value.to === undefined) return "any";
  const quick = value.to === undefined && value.from !== undefined && value.from in RELATIVE_MS;
  return quick ? (value.from ?? "custom") : "custom";
}

const optionsOf = <T extends string>(values: readonly T[], label: (value: T) => string) =>
  values.map((id) => ({ id, label: label(id) }));

/** The choices of a filter that is set or not, led by its "any" choice. */
const withAny = (any: string, options: readonly { id: string; label: string }[]) => [
  { id: ANY, label: any },
  ...options,
];

function findOption<T extends string>(values: readonly T[], id: string | null): T | undefined {
  return values.find((value) => value === id);
}

/** The filters in the "More filters" panel that are set, in words, for the closed panel. */
function appliedText(value: MessageFilters): string | undefined {
  const parts = [
    value.direction && `Direction: ${directionText(value.direction)}`,
    value.state && `State: ${messageStateStatus(value.state).label}`,
    value.delivery && `Delivery: ${deliveryLabel(value.delivery)}`,
    value.idExact && "Identifier: exactly",
    value.messageId && `Message ID: starts with ${value.messageId}`,
  ].filter(Boolean);
  return parts.length === 0 ? undefined : parts.join(" · ");
}

/** The limits of the text and message ID searches, as the toolbar's hint. */
function searchHint({ text, messageId }: MessageFilters): string | undefined {
  const hints = [
    text &&
      (isSearchableText(text)
        ? "Text search covers the selected time range only."
        : `Type at least ${MIN_TEXT} characters to search text.`),
    messageId &&
      !isMessageIdPrefix(messageId) &&
      "A message ID needs 8 to 32 hex digits, dashes optional.",
  ].filter(Boolean);
  return hints.length === 0 ? undefined : hints.join(" ");
}

/**
 * The Messages list's filters (spec §6): identifier (starts with or exact) and text finds
 * after a pause in typing, channels (several), time range, quick filters with counts, and in
 * "More filters" direction, message state and delivery state. Every change reports the whole
 * next set of filters; "Clear filters" shows while any differs from the defaults.
 */
export function MessageFilterBar({ value, onChange, channels, actions }: MessageFilterBarProps) {
  // "Custom range" chosen with no times typed yet: the range control must still show it.
  const [customRange, setCustomRange] = useState(false);
  const range = rangeOf(value, customRange);
  // A find reports after a pause in typing: it must change the filters as they are by then.
  const latest = useLatest(value);
  function change<Key extends keyof MessageFilters>(
    key: Key,
    next: MessageFilters[Key] | undefined,
  ) {
    onChange(withValue(latest.current, key, next));
  }
  /** A message or delivery state choice replaces a quick filter, which stands in for them. */
  function chooseState(key: "state" | "delivery", next: MessageFilters[typeof key]) {
    onChange(withValue(without(latest.current, "quick"), key, next));
  }
  const known = new Set(channels.map((channel) => channel.id));
  const channelOptions = [
    ...channels,
    ...value.channels.filter((id) => !known.has(id)).map((id) => ({ id, label: id })),
  ];
  const channelSummary =
    value.channels.length === 0
      ? "All"
      : value.channels.length === 1
        ? (channelOptions.find((channel) => channel.id === value.channels[0])?.label ??
          "1 selected")
        : `${value.channels.length} selected`;
  const stateOptions = optionsOf(MESSAGE_STATES, (state) => messageStateStatus(state).label);
  return (
    <Toolbar
      variant="collection"
      hint={searchHint(value)}
      moreFilters={{
        applied: appliedText(value),
        content: (
          <>
            <Select
              layout="inline"
              label="Direction"
              name="direction"
              options={withAny("Any direction", optionsOf(MESSAGE_DIRECTIONS, directionText))}
              value={value.direction ?? ANY}
              onChange={(next) => change("direction", findOption(MESSAGE_DIRECTIONS, next))}
            />
            <Select
              layout="inline"
              label="Message state"
              name="state"
              options={withAny("Any message state", stateOptions)}
              value={value.state ?? ANY}
              onChange={(next) => chooseState("state", findOption(MESSAGE_STATES, next))}
            />
            <Select
              layout="inline"
              label="Delivery state"
              name="delivery"
              options={withAny("Any delivery state", optionsOf(DELIVERY_STATES, deliveryLabel))}
              value={value.delivery ?? ANY}
              onChange={(next) => chooseState("delivery", findOption(DELIVERY_STATES, next))}
            />
            <Select
              layout="inline"
              label="Identifier match"
              name="identifier-match"
              options={MATCHES}
              value={value.idExact ? "exact" : "prefix"}
              onChange={(next) => change("idExact", next === "exact" ? true : undefined)}
            />
            <FindField
              label="Message ID"
              visibleLabel="Message ID"
              placeholder="First 8 or more hex digits"
              value={value.messageId ?? ""}
              onChange={(text) => change("messageId", text.trim())}
            />
          </>
        ),
      }}
      actions={
        <>
          {isFiltered(value) && (
            <Button
              variant="quiet"
              onPress={() => {
                setCustomRange(false);
                onChange(DEFAULT_FILTERS);
              }}
            >
              Clear filters
            </Button>
          )}
          {actions}
        </>
      }
    >
      <FindField
        label="Identifier"
        visibleLabel="Identifier"
        placeholder="Control, sample or patient ID"
        value={value.id ?? ""}
        onChange={(text) => change("id", text.trim())}
      />
      <FindField
        label="Message text"
        visibleLabel="Message text"
        placeholder="Text in identifiers, type and bodies"
        value={value.text ?? ""}
        onChange={(text) => change("text", text.trim())}
      />
      <ChecklistPopover
        label="Channels"
        summary={channelSummary}
        options={channelOptions}
        value={value.channels}
        onChange={(channelIds) => onChange({ ...latest.current, channels: channelIds })}
        resetLabel="Show all channels"
      />
      <Select
        layout="inline"
        label="Time range"
        name="range"
        options={RANGES}
        value={range}
        onChange={(next) => {
          setCustomRange(next === "custom");
          if (next === null) return;
          const current = latest.current;
          if (next === "custom") {
            // A quick range becomes the same times, written out, so they can be edited.
            const from = resolveTime(current.from, Date.now());
            if (current.from !== from && from !== undefined) onChange({ ...current, from });
            return;
          }
          const times = without(without(current, "from"), "to");
          onChange(next === "any" ? times : { ...times, from: next });
        }}
      />
      {range === "custom" && (
        <>
          <DateTimeField
            label="From"
            value={value.from}
            onCommit={(from) => change("from", from)}
          />
          <DateTimeField label="To" value={value.to} onCommit={(to) => change("to", to)} />
        </>
      )}
      <QuickFilters
        filters={value}
        onChange={(quick) => {
          const next = without(without(without(latest.current, "quick"), "state"), "delivery");
          onChange(quick === undefined ? next : { ...next, quick });
        }}
      />
    </Toolbar>
  );
}
