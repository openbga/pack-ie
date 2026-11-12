import { X } from "lucide-react";
import { type ReactNode, useId, useState } from "react";
import { Input, Label, SearchField, Button as SearchFieldButton } from "react-aria-components";
import { Button } from "@/components/Button";
import { FindField } from "@/components/FindField";
import { useTypingPause } from "@/components/FindField/useTypingPause";
import { Select } from "@/components/Select";
import { TimeRangeFields } from "@/components/TimeRangeFields";
import { Toolbar } from "@/components/Toolbar";
import { without, withValue } from "@/lib/optionalKeys";
import { useLatest } from "@/lib/useLatest";
import styles from "./LogFilterBar.module.css";
import { LOG_EVENT_NAMES } from "./logEvents";
import { LEVEL_CHOICES } from "./logLevels";
import { DEFAULT_FILTERS, isFiltered, type LogFilters, type LogRange } from "./logParams";

interface Option {
  id: string;
  label: string;
}

interface LogFilterBarProps {
  value: LogFilters;
  /** The whole next set of filters, whenever one changes. */
  onChange: (next: LogFilters) => void;
  channels: readonly Option[];
  endpoints: readonly Option[];
  /** The count of events loaded. */
  count?: ReactNode;
  /** Controls that follow the filters: follow, pause, refresh. */
  actions?: ReactNode;
}

const ANY = "__any";

const RANGES: readonly { id: LogRange; label: string }[] = [
  { id: "live", label: "Live" },
  { id: "15m", label: "Last 15 minutes" },
  { id: "1h", label: "Last hour" },
  { id: "24h", label: "Last 24 hours" },
  { id: "custom", label: "Custom range…" },
];

/** The options plus "All …", and the chosen id when it is no longer one of them. */
function withAll(all: string, options: readonly Option[], chosen: string | undefined): Option[] {
  const list = [{ id: ANY, label: all }, ...options];
  if (chosen !== undefined && !options.some((option) => option.id === chosen)) {
    list.push({ id: chosen, label: chosen });
  }
  return list;
}

/**
 * The Logs filters (CON-05): minimum level, event, channel, endpoint, text and time range. Each
 * change reports the whole next set of filters; the text and the event report after a pause in
 * typing. "Clear filters" restores Info and above, live, with none of the others.
 */
export function LogFilterBar({
  value,
  onChange,
  channels,
  endpoints,
  count,
  actions,
}: LogFilterBarProps) {
  // The text reports after a pause in typing: it must change the filters as they are by then.
  const latest = useLatest(value);
  const change = (next: Partial<LogFilters>) => onChange({ ...latest.current, ...next });
  const choice = (key: "channel" | "endpoint", next: string | null) =>
    onChange(withValue(latest.current, key, next === ANY || next === null ? undefined : next));
  return (
    <Toolbar
      variant="collection"
      count={count}
      actions={
        <>
          {isFiltered(value) && (
            <Button variant="quiet" onPress={() => onChange(DEFAULT_FILTERS)}>
              Clear filters
            </Button>
          )}
          {actions}
        </>
      }
    >
      <FindField
        label="Search log messages"
        placeholder="Text in the message"
        value={value.text ?? ""}
        onChange={(text) => onChange(withValue(latest.current, "text", text.trim()))}
      />
      <Select
        layout="inline"
        label="Level"
        name="level"
        options={LEVEL_CHOICES}
        value={value.level}
        onChange={(next) => {
          const level = LEVEL_CHOICES.find((choiceOption) => choiceOption.id === next);
          if (level) change({ level: level.id });
        }}
      />
      <EventField
        value={value.event ?? ""}
        onChange={(event) => onChange(withValue(latest.current, "event", event.trim()))}
      />
      <Select
        layout="inline"
        label="Channel"
        name="channel"
        options={withAll("All channels", channels, value.channel)}
        value={value.channel ?? ANY}
        onChange={(next) => choice("channel", next)}
      />
      <Select
        layout="inline"
        label="Endpoint"
        name="endpoint"
        options={withAll("All endpoints", endpoints, value.endpoint)}
        value={value.endpoint ?? ANY}
        onChange={(next) => choice("endpoint", next)}
      />
      <TimeRangeFields
        layout="inline"
        ranges={RANGES}
        range={value.range}
        from={value.from}
        to={value.to}
        onRange={(range) => onChange({ ...without(latest.current, "from", "to"), range })}
        onFrom={(from) => onChange(withValue(latest.current, "from", from))}
        onTo={(to) => onChange(withValue(latest.current, "to", to))}
      />
    </Toolbar>
  );
}

interface EventFieldProps {
  /** The event name in effect; empty for any. */
  value: string;
  /** Called after a pause in typing, and at once on Enter, Escape or clear. */
  onChange: (value: string) => void;
}

/**
 * The event name to show, typed or chosen from the catalog's names as suggestions. The engine
 * matches the whole name, so a half-typed name shows nothing until it is complete.
 */
function EventField({ value, onChange }: EventFieldProps) {
  const listId = useId();
  const [text, setText] = useState(value);
  // The filter can change from outside (Back, "Clear filters"): show it then, and drop what was
  // typed but not yet reported. A change the field reported itself keeps the typed text.
  const [shown, setShown] = useState(value);
  const pause = useTypingPause(onChange);
  if (value !== shown) {
    setShown(value);
    if (value !== text.trim()) {
      pause.cancel();
      setText(value);
    }
  }
  return (
    <SearchField
      value={text}
      onChange={(next) => {
        setText(next);
        pause.later(next);
      }}
      onSubmit={pause.now}
      onClear={() => pause.now("")}
      className={styles.eventField}
    >
      <Label className={styles.eventLabel}>Event</Label>
      <div className={styles.eventControl}>
        <Input
          list={listId}
          placeholder="Event name, such as channel.started"
          className={styles.eventInput}
        />
        <SearchFieldButton aria-label="Clear event filter" className={styles.eventClear}>
          <X aria-hidden="true" />
        </SearchFieldButton>
      </div>
      <datalist id={listId}>
        {LOG_EVENT_NAMES.map((name) => (
          <option key={name} value={name} />
        ))}
      </datalist>
    </SearchField>
  );
}
