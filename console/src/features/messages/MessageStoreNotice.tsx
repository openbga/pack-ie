import { useEngineApi } from "@/api";
import type { EngineError, StoreStatus } from "@/api/types";
import { Alert } from "@/components/Alert";
import { EmptyState } from "@/components/EmptyState";

/** Why the store cannot be listed. */
export type StoreProblem = "not_implemented" | "unavailable" | "not_configured";

/**
 * Whether the message store cannot be listed, and why: the engine answered `not_implemented`
 * to a store query, or the snapshot reports the store `unavailable` or `not_configured`.
 * `null` when the store works (or nothing says otherwise yet).
 */
export function storeProblem(
  error: EngineError | null | undefined,
  store: StoreStatus | null | undefined,
): StoreProblem | null {
  if (error?.code === "not_implemented") return "not_implemented";
  if (store?.state === "unavailable" || store?.state === "not_configured") return store.state;
  return null;
}

const REASONS: Record<StoreProblem, string> = {
  not_implemented:
    "This engine does not serve its stored messages yet, so there is nothing to list. Your channels are not affected.",
  unavailable:
    "The engine reports that its message store is unavailable. Messages cannot be listed, and channels cannot store new ones until it works again; senders will retry. See Alerts and Logs for the cause.",
  not_configured:
    "This engine build runs without a message store, so nothing is stored and nothing can be listed. Message channels need a store to receive messages.",
};

/**
 * Shown instead of the Messages list when the store cannot be read. It says what is true and
 * why; nothing failed on this screen, so it is not an error.
 */
export function StoreNotAvailable({ problem }: { problem: StoreProblem }) {
  return <EmptyState title="The message store is not available" description={REASONS[problem]} />;
}

/** Above the Messages screens while the console shows synthetic sample messages. */
export function SampleDataNotice() {
  const { capabilities } = useEngineApi();
  if (capabilities.messages !== "demo") return null;
  return (
    <Alert tone="info" title="Sample messages">
      <p>
        These messages are synthetic sample data. The connected engine does not serve its message
        store to the console yet.
      </p>
    </Alert>
  );
}
