import { useCallback, useMemo, useState } from "react";
import type { ChannelKind, DestinationStatus } from "@/api/types";
import { Alert } from "@/components/Alert";
import { Button } from "@/components/Button";
import { Checkbox } from "@/components/Checkbox";
import { Section } from "@/components/Section";
import { useToast } from "@/components/Toast";
import { formatCount, useFormatTime } from "@/lib/format";
import styles from "./TrafficPanel.module.css";
import { TranscriptPane } from "./TranscriptPane";
import { flowMarker, toHex, toSymbolicText } from "./trafficText";
import { type Feed, type Pane, type TrafficEntry, useTrafficFeed } from "./useTrafficFeed";

/** The panes of a channel, in order, with what each is called (operator-console.md section 7). */
function panesOf(kind: ChannelKind): readonly { id: Pane; title: string }[] {
  return kind === "bridge"
    ? [
        { id: "a_to_b", title: "A → B" },
        { id: "b_to_a", title: "B → A" },
      ]
    : [
        { id: "a_to_b", title: "Source" },
        { id: "b_to_a", title: "Destinations" },
      ];
}

function paneText(
  title: string,
  entries: readonly TrafficEntry[],
  showHex: boolean,
  clock: (at: string) => string,
  labels: ReadonlyMap<string, string> | null,
): string {
  const lines = entries.map(({ segment, bytes }) => {
    const marker = labels === null ? null : flowMarker(segment);
    const id = segment.destinationId ?? null;
    const head = [
      clock(segment.at),
      ...(marker === null ? [] : [`${marker.arrow} ${marker.label}`]),
      ...(labels === null || id === null ? [] : [`[${labels.get(id) ?? id}]`]),
      `${bytes.length} B`,
    ].join("  ");
    return [head, `  ${toSymbolicText(bytes)}`, ...(showHex ? [`  ${toHex(bytes)}`] : [])].join(
      "\n",
    );
  });
  return [`== ${title} ==`, ...lines].join("\n");
}

function transcriptText(
  feed: Feed,
  panes: readonly { id: Pane; title: string }[],
  showHex: boolean,
  clock: (at: string) => string,
  labels: ReadonlyMap<string, string> | null,
): string {
  return panes
    .map((pane) => paneText(pane.title, feed[pane.id].entries, showHex, clock, labels))
    .join("\n\n");
}

/**
 * What the two panes of a message channel mean, and, for segments the engine did not tag with
 * a destination, that the Destinations pane shows them untagged.
 */
function MessagePaneNotes({ destinationEntries }: { destinationEntries: readonly TrafficEntry[] }) {
  const isUntagged =
    destinationEntries.length > 0 &&
    destinationEntries.every((entry) => (entry.segment.destinationId ?? null) === null);
  return (
    <>
      <p className={styles.legend}>
        ← came in to the engine, → went out of it. Source: what the source sent (Received) and the
        engine's answers to it (Response). Destinations: what the engine sent (Sent) and what each
        destination answered (Reply), tagged with the destination.
      </p>
      {isUntagged && (
        <p className={styles.legend}>
          The engine did not tag these Destinations segments with a destination, so they are shown
          untagged.
        </p>
      )}
    </>
  );
}

/**
 * The live transcript of one channel (BRG-05, LOG-02; operator-console.md section 7). Off
 * until the operator turns it on: only then is the engine's tap subscribed, and bodies appear
 * only here, in memory, never stored. A bridge has two panes, `A → B` and `B → A`; a message
 * channel has `Source` and `Destinations`. Each pane keeps the newest 1000 segments, every one
 * timestamped, as text with control characters by name and, on request, as hex. Freeze stops
 * the display while capture goes on.
 */
export function TrafficPanel({
  channelId,
  kind,
  endpointLabels,
  destinations = [],
}: {
  channelId: string;
  kind: ChannelKind;
  /** Bridge sides A and B, named in the legend. */
  endpointLabels: readonly [string, string] | null;
  /** Message channels: the destinations, to name the tag on each Destinations segment. */
  destinations?: readonly Pick<DestinationStatus, "destinationId" | "label">[];
}) {
  const toast = useToast();
  const formatTime = useFormatTime();
  const clock = useCallback(
    (at: string) => formatTime(at, { millis: true, timeOnly: true }),
    [formatTime],
  );
  const transcript = useTrafficFeed(channelId);
  const { isOn, isFrozen, shown } = transcript;
  const [showHex, setShowHex] = useState(false);
  const panes = panesOf(kind);
  const labels = useMemo(
    () =>
      kind === "message"
        ? new Map(destinations.map((destination) => [destination.destinationId, destination.label]))
        : null,
    [kind, destinations],
  );
  const total = shown.a_to_b.entries.length + shown.b_to_a.entries.length;
  const isActive = isOn || total > 0;

  async function copy() {
    try {
      await navigator.clipboard.writeText(transcriptText(shown, panes, showHex, clock, labels));
      toast.show({
        tone: "info",
        title: `Copied ${formatCount(total)} ${total === 1 ? "segment" : "segments"}`,
      });
    } catch {
      toast.show({
        tone: "danger",
        title: "Could not copy the transcript",
        description: "Select the text in the transcript and copy it instead.",
      });
    }
  }

  return (
    <Section title="Live transcript">
      <p className={styles.note}>
        {isOn
          ? "Showing live traffic. Message bodies are shown only in this panel, kept in memory and never stored or logged."
          : "The transcript is off. Turn it on to watch the bytes passing through this channel. Message bodies are shown only in this panel while it is on; they are never stored or logged. One client at a time can watch a channel."}
      </p>
      {transcript.rejection !== null && (
        <Alert tone="warning" title="The transcript could not be started">
          {transcript.rejection}
        </Alert>
      )}
      <div className={styles.controls}>
        <Button
          variant={isOn ? "secondary" : "primary"}
          onPress={isOn ? transcript.stop : transcript.start}
        >
          {isOn ? "Stop live transcript" : "Start live transcript"}
        </Button>
        <Button
          isDisabled={!isOn && !isFrozen}
          onPress={isFrozen ? transcript.unfreeze : transcript.freeze}
        >
          {isFrozen ? "Unfreeze" : "Freeze"}
        </Button>
        <Button isDisabled={total === 0 && !isFrozen} onPress={transcript.clear}>
          Clear
        </Button>
        <Button isDisabled={total === 0} onPress={() => void copy()}>
          Copy
        </Button>
        <Checkbox isSelected={showHex} onChange={setShowHex}>
          Show hex
        </Checkbox>
        {isFrozen && (
          <span className={styles.frozen}>
            {`Display frozen; ${formatCount(transcript.heldBack)} new ${transcript.heldBack === 1 ? "segment" : "segments"} captured.`}
          </span>
        )}
      </div>
      {labels !== null && <MessagePaneNotes destinationEntries={shown.b_to_a.entries} />}
      {endpointLabels && (
        <p className={styles.legend}>
          A = {endpointLabels[0]} · B = {endpointLabels[1]}
        </p>
      )}
      <div className={styles.panes}>
        {panes.map((pane) => (
          <TranscriptPane
            key={pane.id}
            title={pane.title}
            entries={shown[pane.id].entries}
            received={shown[pane.id].received}
            showHex={showHex}
            clock={clock}
            isFrozen={isFrozen}
            isActive={isActive}
            destinationLabels={labels}
          />
        ))}
      </div>
    </Section>
  );
}
