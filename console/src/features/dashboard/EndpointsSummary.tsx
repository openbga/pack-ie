import { Link } from "react-router";
import type { EndpointSnapshot, EndpointState } from "@/api/types";
import { Section } from "@/components/Section";
import { formatAge } from "@/lib/format";
import { ShapeChip } from "../shared-operate/ShapeChip";
import { endpointShape } from "../shared-operate/stateShapes";
import styles from "./Dashboard.module.css";

/** Worst first, so the badges read from what needs attention to what is calm. */
const STATE_ORDER: readonly EndpointState[] = [
  "error",
  "retrying",
  "disconnected",
  "connecting",
  "connected",
  "listening",
  "disabled",
];

/** Endpoints by state, then the unhealthy ones by name, each linking to its page. */
export function EndpointsSummary({
  endpoints,
  unhealthy,
}: {
  endpoints: readonly EndpointSnapshot[];
  unhealthy: readonly EndpointSnapshot[];
}) {
  const counts = STATE_ORDER.map((state) => ({
    state,
    count: endpoints.filter((endpoint) => endpoint.state === state).length,
  })).filter((entry) => entry.count > 0);
  return (
    <Section title="Endpoints">
      {endpoints.length === 0 ? (
        <p className={styles.muted}>
          No endpoints yet. <Link to="/endpoints/new">Add endpoint</Link>
        </p>
      ) : (
        <>
          <ul className={styles.badges} aria-label="Endpoints by state">
            {counts.map(({ state, count }) => {
              const shape = endpointShape(state);
              return (
                <li key={state}>
                  <ShapeChip shape={shape}>{`${count} ${shape.label}`}</ShapeChip>
                </li>
              );
            })}
          </ul>
          {unhealthy.length > 0 && (
            <ul className={styles.rows} aria-label="Endpoints that need attention">
              {unhealthy.slice(0, 5).map((endpoint) => {
                return (
                  <li key={endpoint.id} className={styles.row}>
                    <Link to={`/endpoints/${endpoint.id}`}>{endpoint.label}</Link>
                    <ShapeChip shape={endpointShape(endpoint.state)} />
                    <span className={styles.muted}>since {formatAge(endpoint.stateSince)}</span>
                    {endpoint.lastError && (
                      <span className={styles.rowDetail}>{endpoint.lastError.summary}</span>
                    )}
                  </li>
                );
              })}
            </ul>
          )}
          <Link to="/endpoints">
            {unhealthy.length > 5 ? `View all (${endpoints.length})` : "View endpoints"}
          </Link>
        </>
      )}
    </Section>
  );
}
