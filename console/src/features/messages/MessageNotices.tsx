import { Link } from "react-aria-components";
import type { MessageDetail } from "@/api/types";
import { Alert } from "@/components/Alert";
import {
  MESSAGE_STATE_MEANING,
  messageStateStatus,
  needsAttention,
  reasonWithCode,
} from "./messageWording";

/** The reason a message did not produce deliveries, in words, when the record carries one. */
function reasonOf(detail: MessageDetail): string | null {
  if (detail.abortReason !== null) return reasonWithCode(detail.abortReason);
  return detail.reasonCode === null ? null : reasonWithCode(detail.reasonCode);
}

/**
 * What a person must know first: why a message that is not `routed` has no deliveries (its
 * state explained, with the reason code in words), that a newer revision supersedes it, and how
 * many deliveries need attention.
 */
export function MessageNotices({ detail }: { detail: MessageDetail }) {
  const { state } = detail.summary;
  const reason = reasonOf(detail);
  const unresolved = detail.deliveries.filter((d) => needsAttention(d.state)).length;
  const tone = state === "invalid" || state === "aborted" ? "warning" : "info";
  return (
    <>
      {detail.revisedBy !== null && (
        <Alert tone="warning" title="Superseded by a newer revision">
          <p>
            A newer revision of this message exists:{" "}
            <Link href={`/messages/${encodeURIComponent(detail.revisedBy)}`}>
              Open the newer message
            </Link>
            . Resending this older version may reissue a corrected result, so the resend asks you to
            confirm it.
          </p>
        </Alert>
      )}
      {state !== "routed" && (
        <Alert tone={tone} title={`${messageStateStatus(state).label} message`}>
          <p>{MESSAGE_STATE_MEANING[state]}</p>
          {reason !== null && <p>{reason}</p>}
        </Alert>
      )}
      {unresolved > 0 && (
        <Alert tone="warning" title="Needs attention">
          {unresolved === 1
            ? "One delivery failed, was rejected, or has no confirmed outcome."
            : `${unresolved} deliveries failed, were rejected, or have no confirmed outcome.`}{" "}
          Resend them, or close each delivery with a reason. Closing does not contact the
          destination.
        </Alert>
      )}
    </>
  );
}
