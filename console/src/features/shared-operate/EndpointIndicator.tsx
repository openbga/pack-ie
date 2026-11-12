import { ArrowDownToLine, ArrowUpFromLine, X } from "lucide-react";
import { Link } from "react-router";
import type { EndpointState } from "@/api/types";
import styles from "./EndpointIndicator.module.css";
import { endpointShape } from "./stateShapes";

/** Which way data flows at this side of a `receiver_then_sender` bridge. */
export type FlowArrow = "inbound" | "outbound";

/**
 * One endpoint of a channel: the shape of its state, an optional side name (`A`, `Source`),
 * its label and its state in words. Under `receiver_then_sender` a direction arrow marks the
 * receiver (inbound) and the sender (outbound); a side that is down shows a cross with its
 * arrow. `note` adds what the state alone does not say, such as "waiting for peer".
 */
export function EndpointIndicator({
  side,
  label,
  state,
  arrow = null,
  note = null,
  href,
}: {
  /** Which side of the channel this is, such as `A`, `Side B` or `Source`. */
  side?: string;
  label: string;
  /** Makes the label a link to the endpoint's page. */
  href?: string;
  state: EndpointState;
  arrow?: FlowArrow | null;
  note?: string | null;
}) {
  const shape = endpointShape(state);
  const down = state === "disconnected" || state === "error";
  const Icon = arrow !== null && down ? X : shape.Icon;
  const Arrow = arrow === "inbound" ? ArrowDownToLine : ArrowUpFromLine;
  return (
    <span className={styles.indicator} data-tone={shape.tone}>
      <Icon aria-hidden="true" className={styles.icon} />
      {arrow !== null && (
        <Arrow
          role="img"
          aria-label={arrow === "inbound" ? "Receives" : "Sends"}
          className={styles.icon}
        />
      )}
      {side !== undefined && (
        <>
          <span className={styles.role}>{side}</span>{" "}
        </>
      )}
      <span className={styles.name} title={label}>
        {href ? <Link to={href}>{label}</Link> : label}
      </span>{" "}
      <span className={styles.state}>
        {shape.label}
        {note !== null && `, ${note}`}
      </span>
    </span>
  );
}
