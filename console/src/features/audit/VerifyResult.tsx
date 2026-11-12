import type { AuditVerification } from "@/api/types";
import { Alert } from "@/components/Alert";
import { Facts } from "@/components/Facts";
import { formatCount } from "@/lib/format";
import layout from "@/styles/layout.module.css";
import { PROBLEM_LABELS } from "./auditLabels";

const number = (value: number | null) => (value === null ? "None" : formatCount(value));

/** The chain's first broken event, for the title. */
function firstBrokenSeq(result: AuditVerification): number | null {
  return result.problems.find((problem) => problem.seq !== null)?.seq ?? null;
}

/**
 * What "Verify chain" found, in a named region: intact with the head hash to compare with the
 * engine log's last `audit.anchor` line, or broken with each problem in words.
 */
export function VerifyResult({
  result,
  onDismiss,
}: {
  result: AuditVerification;
  onDismiss: () => void;
}) {
  const broken = firstBrokenSeq(result);
  const title = result.ok
    ? "Chain verified: OK"
    : broken === null
      ? "Chain broken"
      : `Chain broken at seq ${broken}`;
  return (
    <section aria-label="Chain verification">
      <Alert tone={result.ok ? "success" : "danger"} title={title} onDismiss={onDismiss}>
        <div className={layout.stack}>
          <Facts
            layout="compact"
            items={[
              { label: "Files", value: number(result.files) },
              { label: "Events", value: number(result.events) },
              { label: "First event", value: number(result.firstSeq) },
              { label: "Last event", value: number(result.lastSeq) },
            ]}
          />
          <Facts
            items={[
              {
                label: "Head hash",
                value:
                  result.headHash === null ? (
                    "None"
                  ) : (
                    <code className={layout.code}>{result.headHash}</code>
                  ),
              },
            ]}
          />
          {result.ok && result.firstSeq !== null && result.firstSeq > 1 && (
            <p>
              The chain starts at event {result.firstSeq} because retention removed older files
              (recorded as audit_prune); the rest is intact.
            </p>
          )}
          {!result.ok && (
            <>
              <p>
                {result.problems.length === 1
                  ? "1 problem found:"
                  : `${result.problems.length} problems found:`}
              </p>
              <ul className={layout.list}>
                {result.problems.map((problem) => (
                  <li key={`${problem.file}:${problem.seq}:${problem.kind}`}>
                    <strong>{PROBLEM_LABELS[problem.kind]}</strong>
                    {" · "}
                    {problem.seq === null ? "no seq" : `seq ${problem.seq}`}
                    {" · "}
                    <code className={layout.code}>{problem.file}</code>
                    {problem.message !== "" && <> · {problem.message}</>}
                  </li>
                ))}
              </ul>
            </>
          )}
          <p className={layout.hint}>
            Compare the head hash with the last <code className={layout.code}>audit.anchor</code>{" "}
            line of the engine log: a chain shortened at its newest end still verifies, and the
            anchor shows it. Verifying is recorded in the audit log.
          </p>
        </div>
      </Alert>
    </section>
  );
}
