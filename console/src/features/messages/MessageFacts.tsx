import { Link } from "react-aria-components";
import type { MessageDetail } from "@/api/types";
import { Facts } from "@/components/Facts";
import { Timestamp } from "@/components/Timestamp";
import { formatBytes } from "@/lib/format";
import { protocolLabel } from "@/lib/status";
import { directionText, originText } from "./messageWording";

const NOT_PROVIDED = "Not provided";

/** A link to another stored message. */
function MessageLink({ id, children }: { id: string; children: string }) {
  return <Link href={`/messages/${encodeURIComponent(id)}`}>{children}</Link>;
}

/** The linked messages of a record: original of a duplicate, source of a resend, its revisions. */
function links({ duplicateOf, summary, revisionOf, revisedBy }: MessageDetail) {
  const items: { label: string; id: string; text: string }[] = [];
  if (duplicateOf)
    items.push({ label: "Duplicate of", id: duplicateOf, text: "The original message" });
  if (summary.resentFrom) {
    items.push({ label: "Resent from", id: summary.resentFrom, text: "The first message" });
  }
  if (revisionOf) items.push({ label: "Revision of", id: revisionOf, text: "The earlier message" });
  if (revisedBy) items.push({ label: "Superseded by", id: revisedBy, text: "The newest revision" });
  return items.map(({ label, id, text }) => ({
    label,
    value: <MessageLink id={id}>{text}</MessageLink>,
  }));
}

/** Where the message came from: the source endpoint, its peer or the file and rows. */
function sourceFacts(detail: MessageDetail) {
  const { sourceFile, summary } = detail;
  return [
    { label: "Source endpoint", value: detail.sourceEndpointLabel },
    { label: "Source peer", value: summary.sourcePeer ?? NOT_PROVIDED },
    ...(sourceFile === null
      ? []
      : [
          {
            label: "Source file",
            value:
              sourceFile.firstRow === sourceFile.lastRow
                ? `${sourceFile.name}, row ${sourceFile.firstRow}`
                : `${sourceFile.name}, rows ${sourceFile.firstRow} to ${sourceFile.lastRow}`,
          },
        ]),
  ];
}

/** The response sent to the sender, with its time (spec §6 Detail). */
function responseFact(detail: MessageDetail) {
  const { responseCode, respondedAt } = detail;
  return {
    label: "Response sent to the sender",
    value:
      responseCode === null ? (
        "None sent"
      ) : (
        <>
          {responseCode}
          {respondedAt !== null && (
            <>
              {" at "}
              <Timestamp value={respondedAt} seconds />
            </>
          )}
        </>
      ),
  };
}

/**
 * The record's facts: identifiers, times, source, configuration version, the response sent
 * to the sender and the links to related messages. An identifier the protocol did not
 * provide says so.
 */
export function MessageFacts({ detail }: { detail: MessageDetail }) {
  const { summary } = detail;
  return (
    <Facts
      layout="compact"
      items={[
        { label: "Received", value: <Timestamp value={summary.receivedAt} seconds /> },
        {
          label: "Stored",
          value:
            detail.committedAt === null ? (
              "Not complete yet"
            ) : (
              <Timestamp value={detail.committedAt} seconds />
            ),
        },
        {
          label: "Channel",
          value: (
            <Link href={`/channels/${encodeURIComponent(summary.channelId)}`}>
              {summary.channelLabel}
            </Link>
          ),
        },
        { label: "Direction", value: directionText(summary.direction) },
        { label: "Origin", value: originText(summary.origin) },
        { label: "Protocol", value: protocolLabel(summary.protocol) },
        { label: "Message type", value: summary.messageType ?? NOT_PROVIDED },
        { label: "Control ID", value: summary.controlId ?? NOT_PROVIDED },
        { label: "Sample ID", value: summary.sampleId ?? NOT_PROVIDED },
        { label: "Patient ID", value: summary.patientId ?? NOT_PROVIDED },
        { label: "Size", value: formatBytes(summary.sizeBytes) },
        { label: "Message ID", value: summary.id },
        { label: "Sequence in the channel", value: summary.seq },
        ...sourceFacts(detail),
        { label: "Configuration version", value: summary.configVersion },
        responseFact(detail),
        ...links(detail),
      ]}
    />
  );
}
