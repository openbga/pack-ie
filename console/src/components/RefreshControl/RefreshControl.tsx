import { RefreshCw } from "lucide-react";
import { Button } from "@/components/Button";
import { useFormatTime } from "@/lib/format";
import styles from "./RefreshControl.module.css";

/** The parts of an engine query the control shows and runs; any `useEngineQuery` result fits. */
interface RefreshableQuery {
  /** When the data shown was read, in milliseconds since the epoch; 0 before the first read. */
  dataUpdatedAt: number;
  isFetching: boolean;
  refetch: () => unknown;
}

/**
 * When the data shown was read ("Updated 14:03 CDT", announced politely when it changes)
 * and a Refresh button, for lists that change while the screen is open.
 */
export function RefreshControl({ query }: { query: RefreshableQuery }) {
  const formatTime = useFormatTime();
  const updated =
    query.dataUpdatedAt > 0
      ? `Updated ${formatTime(new Date(query.dataUpdatedAt).toISOString(), { today: new Date() })}`
      : "";
  return (
    <div className={styles.control}>
      <span role="status" className={styles.updated}>
        {updated}
      </span>
      <Button isPending={query.isFetching} onPress={() => void query.refetch()}>
        <RefreshCw aria-hidden="true" />
        Refresh
      </Button>
    </div>
  );
}
