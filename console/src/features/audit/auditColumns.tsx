import { Link } from "react-router";
import type { AuditEvent } from "@/api/types";
import type { DataGridColumn } from "@/components/DataGrid";
import { StatusBadge } from "@/components/StatusBadge";
import { Timestamp } from "@/components/Timestamp";
import styles from "./auditColumns.module.css";
import {
  ACTION_LABELS,
  OBJECT_TYPE_LABELS,
  ORIGIN_LABELS,
  OUTCOME_LABELS,
  OUTCOME_TONES,
  objectHref,
} from "./auditLabels";
import { ALL_ORDER, type AuditColumnId, COLUMN_WIDTH, DEFAULT_ORDER } from "./columnFit";

/** The object an event concerns: its type and id, the id linking to its page where there is one. */
export function ObjectCell({ event }: { event: AuditEvent }) {
  if (event.objectType === null) return null;
  const type = OBJECT_TYPE_LABELS[event.objectType];
  const href = objectHref(event);
  if (event.objectId === null || event.objectId === "") return type;
  return (
    <span title={`${type} ${event.objectId}`}>
      {type} {href === null ? event.objectId : <Link to={href}>{event.objectId}</Link>}
    </span>
  );
}

export function OutcomeBadge({ event }: { event: AuditEvent }) {
  return (
    <StatusBadge tone={OUTCOME_TONES[event.outcome]}>{OUTCOME_LABELS[event.outcome]}</StatusBadge>
  );
}

/**
 * Time, who, from where, what, to what and how it ended; the raw action code is in the detail.
 * Time is compact (the full time and zone are its title) and the account is cut short with its
 * full name on hover. Action and Object wrap onto a second line rather than lose their end.
 */
const COLUMNS: Record<AuditColumnId, DataGridColumn<AuditEvent>> = {
  time: {
    id: "time",
    header: "Time",
    identifier: true,
    width: COLUMN_WIDTH.time,
    cell: (event) => <Timestamp value={event.at} compact seconds />,
  },
  actor: {
    id: "actor",
    header: "Actor",
    width: COLUMN_WIDTH.actor,
    cell: (event) => <span title={event.actor}>{event.actor}</span>,
  },
  origin: {
    id: "origin",
    header: "Origin",
    width: COLUMN_WIDTH.origin,
    cell: (event) => ORIGIN_LABELS[event.origin],
  },
  action: {
    id: "action",
    header: "Action",
    grow: 2,
    wrap: true,
    cell: (event) => (
      <span className={styles.clamp} title={ACTION_LABELS[event.action]}>
        {ACTION_LABELS[event.action]}
      </span>
    ),
  },
  object: {
    id: "object",
    header: "Object",
    grow: 2,
    wrap: true,
    cell: (event) => (
      <span className={styles.clamp}>
        <ObjectCell event={event} />
      </span>
    ),
  },
  outcome: {
    id: "outcome",
    header: "Outcome",
    width: COLUMN_WIDTH.outcome,
    cell: (event) => <OutcomeBadge event={event} />,
  },
};

/** Time, Action and Outcome are never hidden. */
const ALWAYS_SHOWN: ReadonlySet<string> = new Set<AuditColumnId>(["time", "action", "outcome"]);

/** What the column chooser offers: the columns that can go. Origin is only here and in the detail. */
export const COLUMN_CHOICES = ALL_ORDER.filter((id) => !ALWAYS_SHOWN.has(id)).map((id) => ({
  id,
  label: COLUMNS[id].header,
}));

/** The grid's columns among `ids`, in display order, and always the ones that stay. */
export function auditColumns(ids: readonly string[]): DataGridColumn<AuditEvent>[] {
  return ALL_ORDER.filter((id) => ALWAYS_SHOWN.has(id) || ids.includes(id)).map(
    (id) => COLUMNS[id],
  );
}

/** The chooser's ids among `ids`: the columns shown that a person can remove. */
export const choosableIn = (ids: readonly string[]): string[] =>
  COLUMN_CHOICES.filter((choice) => ids.includes(choice.id)).map((choice) => choice.id);

/** How many columns the default set has, for saying how many did not fit. */
export const DEFAULT_COLUMN_COUNT = DEFAULT_ORDER.length;
