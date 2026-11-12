import type { ReactNode } from "react";
import type { EndpointKind, EngineError, ProbeReport, ProbeStep } from "@/api/types";
import { Alert } from "@/components/Alert";
import { ErrorState } from "@/components/ErrorState";
import { LoadingState } from "@/components/LoadingState";
import { StatusBadge } from "@/components/StatusBadge";
import styles from "./ProbePanel.module.css";
import { explanationFor, formatMillis, hintFor, stepWords } from "./probeText";

function capitalized(text: string): string {
  return `${text.charAt(0).toUpperCase()}${text.slice(1)}`;
}

function StepRow({ step }: { step: ProbeStep }) {
  const hint = step.ok ? null : hintFor(step);
  return (
    <tr>
      <th scope="row">{stepWords(step.name)}</th>
      <td>
        <StatusBadge tone={step.ok ? "success" : "danger"}>
          {step.ok ? "Passed" : "Failed"}
        </StatusBadge>
      </td>
      <td>
        {capitalized(step.detail)}
        {hint && <p className={styles.hint}>{hint}</p>}
      </td>
      <td data-align="end">{formatMillis(step.millis)}</td>
    </tr>
  );
}

function Report({ report }: { report: ProbeReport }) {
  return (
    <div className={styles.result}>
      <Alert
        tone={report.ok ? "success" : "danger"}
        title={report.ok ? "Test passed" : "Test failed"}
      >
        <p className={styles.summary}>{report.summary}</p>
      </Alert>
      <div className={styles.scroll}>
        <table aria-label="Test steps" className={styles.table}>
          <thead>
            <tr>
              <th scope="col">Step</th>
              <th scope="col">Result</th>
              <th scope="col">What was found</th>
              <th scope="col" data-align="end">
                Time
              </th>
            </tr>
          </thead>
          <tbody>
            {report.steps.map((step) => (
              <StepRow key={step.name} step={step} />
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

interface ProbePanelProps {
  /** What kind of endpoint is tested: the explanation under the title says what that test does. */
  kind: EndpointKind;
  /** The button that runs the test; it sits under the explanation. */
  action: ReactNode;
  /** What is particular to the place the panel is shown, such as a running endpoint. */
  note?: ReactNode;
  isPending: boolean;
  /** The last test's report, if it ran. */
  report: ProbeReport | null;
  /** Why the last test did not run or finish: rejected settings, or the engine. */
  error: EngineError | null;
  /** The form is missing required settings, so no test was started. */
  isIncomplete?: boolean;
  /** The settings changed since the result shown. */
  isStale?: boolean;
  /** Rejected settings are marked on a form beside the panel; otherwise the panel lists them. */
  isOnForm?: boolean;
  onRetry: () => void;
}

function Outcome({
  isPending,
  report,
  error,
  isIncomplete,
  isStale,
  isOnForm,
  onRetry,
}: Omit<ProbePanelProps, "action" | "note" | "kind">) {
  if (isPending) return <LoadingState label="Testing…" />;
  if (isIncomplete) {
    return (
      <Alert tone="warning" title="Fill in the required settings first">
        <p>
          Nothing was tested. The settings that are missing or not valid are marked on the form.
        </p>
      </Alert>
    );
  }
  if (error?.code === "validation") {
    return (
      <Alert tone="danger" title="Settings rejected: no test ran">
        <p>
          The engine rejected these settings before any test ran, so nothing was opened or
          connected.
          {isOnForm ? " The problems are marked on the form." : " Edit the endpoint to fix them:"}
        </p>
        {!isOnForm && (
          <ul>
            {error.fieldErrors.map((problem) => (
              <li key={`${problem.field}|${problem.message}`}>{problem.message}</li>
            ))}
          </ul>
        )}
        {isStale && <p>These settings changed since the test.</p>}
      </Alert>
    );
  }
  if (error) return <ErrorState title="The test could not run" error={error} onRetry={onRetry} />;
  if (report === null) return null;
  return (
    <>
      {isStale && (
        <Alert tone="warning" compact>
          These settings changed since the test. Run it again to test the current ones.
        </Alert>
      )}
      <Report report={report} />
    </>
  );
}

/**
 * "Test connection" for an endpoint, where the endpoint's settings are shown: what the test
 * does and does not do, the button, and the last result as a summary sentence and the steps
 * with their outcome and time. The caller runs the test and passes what came back.
 */
export function ProbePanel({ kind, action, note, ...outcome }: ProbePanelProps) {
  return (
    <div className={styles.panel}>
      <p className={styles.help}>{explanationFor(kind)}</p>
      {note}
      <div>{action}</div>
      <Outcome {...outcome} />
    </div>
  );
}
