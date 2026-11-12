import { Play } from "lucide-react";
import { type ReactNode, useRef } from "react";
import { useConnection, useConnectionSync, useEngineMutation } from "@/api/hooks";
import { Alert } from "@/components/Alert";
import { Button } from "@/components/Button";
import { EmptyState } from "@/components/EmptyState";
import { ErrorState } from "@/components/ErrorState";
import { LoadingState } from "@/components/LoadingState";
import { useToast } from "@/components/Toast";
import styles from "./EngineGate.module.css";
import { startServiceHint } from "./startServiceHint";

/**
 * CON-01: the console attaches to a running engine. Until one answers, say so, say where the
 * console looked, and offer to start the installed service; the console keeps trying and
 * continues by itself. Once the engine has answered, a later loss of the connection keeps the
 * pages (with their last data) and shows a banner above them instead.
 */
export function EngineGate({ children }: { children: ReactNode }) {
  useConnectionSync();
  const connection = useConnection();
  const everConnected = useRef(false);
  if (connection.data?.state === "connected") everConnected.current = true;
  if (connection.isPending) {
    return (
      <div className={styles.gate}>
        <LoadingState label="Connecting to the engine…" />
      </div>
    );
  }
  if (connection.isError) {
    return (
      <div className={styles.gate}>
        <ErrorState
          title="The engine could not be reached"
          error={connection.error}
          onRetry={() => void connection.refetch()}
        />
      </div>
    );
  }
  const status = connection.data;
  if (everConnected.current) {
    // One frame whether or not the connection is up, so the pages keep their place in the tree
    // (and their state) across an outage.
    return (
      <div className={styles.frame}>
        {status.state !== "connected" && (
          <div className={styles.banner}>
            <Alert tone="danger" title="Lost the connection to the engine">
              <p>
                {status.state === "reconnecting"
                  ? "Reconnecting…"
                  : "The console is trying again and will continue by itself."}{" "}
                What is shown may be out of date.
                {status.detail ? ` (${status.detail})` : ""}
              </p>
            </Alert>
          </div>
        )}
        <div className={styles.content}>{children}</div>
      </div>
    );
  }
  if (status.state === "connected") return children;
  return (
    <div className={styles.gate}>
      <NotRunning endpoint={status.endpoint} detail={status.detail} />
    </div>
  );
}

function NotRunning({ endpoint, detail }: { endpoint: string | null; detail: string | null }) {
  const toast = useToast();
  const start = useEngineMutation((api) => api.startService());
  const where = endpoint ? `No engine answered at ${endpoint}.` : "No engine location is known.";
  const hint = startServiceHint(navigator.userAgent);
  return (
    <>
      <EmptyState
        title="The engine is not running"
        description={`${where}${detail ? ` ${detail}` : ""} The console keeps trying and continues as soon as an engine answers.`}
        action={
          <Button
            variant="primary"
            isPending={start.isPending}
            onPress={() =>
              start.mutate(undefined, {
                onSuccess: (text) =>
                  toast.show({ tone: "info", title: "Start service", description: text }),
              })
            }
          >
            <Play aria-hidden="true" />
            Start the service
          </Button>
        }
      />
      <p className={styles.hint}>
        {hint.why} If it does not start from here, run this in an administrator terminal:
      </p>
      <code className={styles.command}>{hint.command}</code>
      {start.error && (
        <Alert tone="warning" title="The service was not started">
          <p>{start.error.message}</p>
        </Alert>
      )}
    </>
  );
}
