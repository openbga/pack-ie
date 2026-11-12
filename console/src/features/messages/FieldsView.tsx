import { useState } from "react";
import type { MessageFields } from "@/api/types";
import { FindField, findRows } from "@/components/FindField";
import styles from "./FieldsView.module.css";
import { scrollRegion } from "./scrollRegion";

/** What the view says about itself, always: the engine read the fields, not the console. */
const FIELDS_SOURCE = "Read by the engine from the original message.";

/** What the engine says when it lists no field and gives no reason. */
const NOTHING = "The engine listed no fields for this message.";

/**
 * The fields the engine read from the original message, as a read-only table of path and value
 * with a box to find one in a long list. The console reads nothing itself: when the engine
 * lists none it says why in the engine's words. Fields are message content, so this is drawn
 * only where the original message was opened.
 */
export function FieldsView({ fields, name }: { fields: MessageFields; name: string }) {
  const [query, setQuery] = useState("");
  const shown = findRows(fields.fields, query, (field) => [field.path, field.value]);
  return (
    <>
      <p className={styles.note}>{FIELDS_SOURCE}</p>
      {fields.unavailable !== null && <p className={styles.note}>{fields.unavailable}</p>}
      {fields.unavailable === null && fields.fields.length === 0 && (
        <p className={styles.note}>{NOTHING}</p>
      )}
      {fields.fields.length > 0 && (
        <>
          <FindField
            label={`Find in ${name.toLowerCase()} fields`}
            placeholder="Path or value"
            value={query}
            onChange={setQuery}
            count={{
              shown: shown.length,
              total: fields.fields.length,
              noun: "fields",
              one: "field",
            }}
          />
          {fields.truncated && <p className={styles.note}>More fields exist than are listed.</p>}
          {shown.length === 0 ? (
            <p className={styles.note}>No field has a path or value containing '{query.trim()}'.</p>
          ) : (
            <div {...scrollRegion(`${name} as fields`)} className={styles.scroll}>
              <table className={styles.table}>
                <thead>
                  <tr>
                    <th scope="col">Path</th>
                    <th scope="col">Value</th>
                  </tr>
                </thead>
                <tbody>
                  {shown.map((field) => (
                    <tr key={field.path}>
                      <th scope="row" className={styles.path}>
                        {field.path}
                      </th>
                      <td className={styles.value}>{field.value}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </>
      )}
    </>
  );
}
