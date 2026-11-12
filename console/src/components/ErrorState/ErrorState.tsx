import type { EngineError } from "@/api/types";
import { Alert } from "@/components/Alert";
import { Button } from "@/components/Button";
import { toUserMessage } from "@/lib/errors";
import styles from "./ErrorState.module.css";

interface ErrorStateProps {
  title?: string;
  error: EngineError;
  onRetry?: () => void;
  /** The retry is running: the button shows it and ignores further presses. */
  isRetrying?: boolean;
}

/**
 * A failed engine call: the safe message and the support reference that finds the matching
 * log line (omitted when the failure never reached the engine), with a retry.
 */
export function ErrorState({
  title = "This could not be loaded",
  error,
  onRetry,
  isRetrying = false,
}: ErrorStateProps) {
  return (
    <Alert tone="danger" title={title}>
      <p>{toUserMessage(error)}</p>
      {error.correlationId && (
        <p className={styles.reference}>
          Support reference: <span className={styles.id}>{error.correlationId}</span>
        </p>
      )}
      {onRetry && (
        <div className={styles.retry}>
          <Button isPending={isRetrying} onPress={onRetry}>
            Try again
          </Button>
        </div>
      )}
    </Alert>
  );
}
