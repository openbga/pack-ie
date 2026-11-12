import { useState } from "react";
import { useEngineMutation } from "@/api/hooks";
import type { EngineError, NotificationResult } from "@/api/types";
import { Alert } from "@/components/Alert";
import { Button } from "@/components/Button";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { ErrorState } from "@/components/ErrorState";
import { Facts } from "@/components/Facts";
import { LoadingState } from "@/components/LoadingState";
import { Section } from "@/components/Section";
import { StatusBadge } from "@/components/StatusBadge";
import styles from "@/styles/layout.module.css";
import { targetName } from "./NotificationsTabTargets";

const AUDIT_REFUSAL =
  "Not sent: the audit log cannot be written, and test messages are audited. Fix the audit log first (see Alerts), then try again.";

/** The refusal when the audit log is down is told in words; any other failure in the engine's. */
function failure(error: EngineError): { title: string; error: EngineError } {
  if (error.code === "unavailable") {
    return { title: "The test message was not sent", error: { ...error, message: AUDIT_REFUSAL } };
  }
  return { title: "The test could not be run", error };
}

function summaryOf(results: readonly NotificationResult[]): {
  tone: "success" | "warning" | "danger";
  title: string;
} {
  const accepted = results.filter((each) => each.ok).length;
  const title = `${accepted} of ${results.length} ${results.length === 1 ? "target" : "targets"} accepted the test message`;
  if (accepted === results.length) return { tone: "success", title };
  return { tone: accepted === 0 ? "danger" : "warning", title };
}

function TestResults({ results }: { results: readonly NotificationResult[] }) {
  if (results.length === 0) {
    return (
      <section aria-label="Test result">
        <Alert tone="info" title="No target to send to">
          No notification target is configured on the engine. It reads its targets from the
          bootstrap file when it starts: after a change, restart the engine.
        </Alert>
      </section>
    );
  }
  const { tone, title } = summaryOf(results);
  return (
    <section aria-label="Test result" className={styles.stack}>
      <Alert tone={tone} title={title}>
        {tone === "success"
          ? "A target that was failing is no longer failing."
          : "A failed send is not retried. Fix the cause, then send another test message."}
      </Alert>
      <Facts
        items={results.map((each) => ({
          label: targetName(each.target),
          value: (
            <>
              {each.ok ? (
                <StatusBadge tone="success">Accepted</StatusBadge>
              ) : (
                <StatusBadge tone="danger">Failed</StatusBadge>
              )}{" "}
              {each.detail}
            </>
          ),
        }))}
      />
    </section>
  );
}

/**
 * Sends one test message to every target the engine has, after saying what that does, and
 * keeps each target's answer on screen until the next test. `targets` are the ones the
 * bootstrap lists, in words: empty when it lists none, undefined when the console cannot tell.
 */
export function NotificationTestSection({
  targets,
  isLoading,
}: {
  targets: readonly string[] | undefined;
  /** The bootstrap is still being read. */
  isLoading: boolean;
}) {
  const [confirming, setConfirming] = useState(false);
  const test = useEngineMutation((api) => api.testNotification(), {
    invalidates: ["listAlerts", "getAuditEvents"],
  });
  const off = !isLoading && targets?.length === 0;
  const failed = test.error === null ? null : failure(test.error);
  function run() {
    test.mutate();
  }
  return (
    <Section
      title="Send a test message"
      actions={
        <Button
          variant="primary"
          isDisabled={isLoading || off}
          isPending={test.isPending}
          onPress={() => setConfirming(true)}
        >
          Send test…
        </Button>
      }
    >
      <p className={styles.hint}>
        {off
          ? "Disabled: no target is configured in the bootstrap file, so there is nothing to send to."
          : "Sends one message to every target and shows what each one answered."}
      </p>
      {test.isPending && <LoadingState label="Sending the test message…" />}
      {failed && (
        <ErrorState title={failed.title} error={failed.error} onRetry={run} isRetrying={false} />
      )}
      {test.data && !test.isPending && <TestResults results={test.data} />}
      <ConfirmDialog
        title="Send a test message?"
        isOpen={confirming}
        onOpenChange={setConfirming}
        confirmLabel="Send test message"
        variant="primary"
        onConfirm={() => {
          setConfirming(false);
          run();
        }}
      >
        <div className={styles.stack}>
          {targets === undefined ? (
            <p>A test message goes to every notification target the engine has.</p>
          ) : (
            <>
              <p>A test message goes to every notification target:</p>
              <ul className={styles.list}>
                {targets.map((each) => (
                  <li key={each}>{each}</li>
                ))}
              </ul>
            </>
          )}
          <p>
            The message has the code test and the severity info. Nothing from messages or channels
            is in it. It is recorded in the audit log with your account (notification_test). A
            failed send is not retried.
          </p>
        </div>
      </ConfirmDialog>
    </Section>
  );
}
