import type { ReactNode } from "react";
import { Button } from "@/components/Button";
import { FindField } from "@/components/FindField";
import { Select } from "@/components/Select";
import { TimeRangeFields } from "@/components/TimeRangeFields";
import { Toolbar } from "@/components/Toolbar";
import { without, withValue } from "@/lib/optionalKeys";
import { ACTION_CHOICES, type Choice, OBJECT_TYPE_CHOICES, OUTCOME_CHOICES } from "./auditLabels";
import { type AuditView, DEFAULT_VIEW, isFiltered, RANGE_CHOICES } from "./auditParams";
import type { ViewChange } from "./useAuditView";

interface AuditFilterBarProps {
  value: AuditView;
  /**
   * The next set of filters, whenever one changes: given as what to change in the set as it is
   * by then, so a change never undoes one just before it that the page has not drawn yet.
   */
  onChange: (change: ViewChange) => void;
  /** The count of events loaded. */
  count?: ReactNode;
  /** Controls after the filters: export, verify, refresh. */
  actions?: ReactNode;
}

const ANY = "__any";

function withAll<Code extends string>(all: string, options: readonly Choice<Code>[]) {
  return [{ id: ANY, label: all }, ...options];
}

/**
 * The Audit filters: time range, actor, action, object type, object ID and outcome. Each change
 * reports the whole next set; typed text reports after a pause. Actor and object ID match
 * exactly, which the fields say.
 */
export function AuditFilterBar({ value, onChange, count, actions }: AuditFilterBarProps) {
  // Text reports after a pause in typing: it must change the filters as they are by then.
  const setText = (key: "actor" | "objectId", text: string) =>
    onChange((current) => withValue(current, key, text.trim()));
  function setChoice<Key extends "action" | "objectType" | "outcome">(
    key: Key,
    options: readonly Choice<NonNullable<AuditView[Key]>>[],
    next: string | null,
  ) {
    const choice = options.find((option) => option.id === next);
    onChange((current) => (choice ? { ...current, [key]: choice.id } : without(current, key)));
  }
  return (
    <Toolbar
      variant="collection"
      count={count}
      actions={
        <>
          {isFiltered(value) && (
            <Button variant="quiet" onPress={() => onChange(DEFAULT_VIEW)}>
              Clear filters
            </Button>
          )}
          {actions}
        </>
      }
    >
      <TimeRangeFields
        layout="inline"
        ranges={RANGE_CHOICES}
        range={value.range}
        from={value.from}
        to={value.to}
        onRange={(range) => onChange((current) => ({ ...without(current, "from", "to"), range }))}
        onFrom={(from) => onChange((current) => withValue(current, "from", from))}
        onTo={(to) => onChange((current) => withValue(current, "to", to))}
      />
      <FindField
        label="Actor"
        visibleLabel="Actor"
        placeholder="Exact account name or uid/SID"
        value={value.actor ?? ""}
        onChange={(text) => setText("actor", text)}
      />
      <Select
        layout="inline"
        label="Action"
        name="action"
        options={withAll("All actions", ACTION_CHOICES)}
        value={value.action ?? ANY}
        onChange={(next) => setChoice("action", ACTION_CHOICES, next)}
      />
      <Select
        layout="inline"
        label="Object type"
        name="objectType"
        options={withAll("All object types", OBJECT_TYPE_CHOICES)}
        value={value.objectType ?? ANY}
        onChange={(next) => setChoice("objectType", OBJECT_TYPE_CHOICES, next)}
      />
      <FindField
        label="Object ID"
        visibleLabel="Object ID"
        placeholder="Exact ID, such as ch-1"
        value={value.objectId ?? ""}
        onChange={(text) => setText("objectId", text)}
      />
      <Select
        layout="inline"
        label="Outcome"
        name="outcome"
        options={withAll("All outcomes", OUTCOME_CHOICES)}
        value={value.outcome ?? ANY}
        onChange={(next) => setChoice("outcome", OUTCOME_CHOICES, next)}
      />
    </Toolbar>
  );
}
