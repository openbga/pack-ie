import { Link } from "react-router";
import { useEngineQuery, useSnapshot } from "@/api/hooks";
import type { EngineError, MessageSummary } from "@/api/types";
import { Button } from "@/components/Button";
import { ErrorState } from "@/components/ErrorState";
import { LoadingState } from "@/components/LoadingState";
import { type MiniColumn, MiniTable } from "@/components/MiniTable";
import { Section } from "@/components/Section";
import { StatusBadge } from "@/components/StatusBadge";
import { Timestamp } from "@/components/Timestamp";
import { DeliveryChips } from "../messages/DeliveryChips";
import { DEFAULT_SORT, toMessageQuery } from "../messages/messageParams";
import { messageStateStatus } from "../messages/messageWording";
import styles from "./ChannelSections.module.css";

/** How many messages the page shows; the Messages list has the rest. */
const RECENT_MESSAGES = 10;

const NOT_PROVIDED = <span className={styles.muted}>Not provided</span>;

const COLUMNS: readonly MiniColumn<MessageSummary>[] = [
  {
    id: "receivedAt",
    header: "Received",
    cell: (message) => (
      <Link to={`/messages/${encodeURIComponent(message.id)}`}>
        <Timestamp value={message.receivedAt} compact seconds />
      </Link>
    ),
  },
  {
    id: "identifier",
    header: "Identifier",
    cell: (message) => message.controlId ?? message.sampleId ?? NOT_PROVIDED,
  },
  {
    id: "state",
    header: "Message state",
    cell: (message) => {
      const status = messageStateStatus(message.state);
      return <StatusBadge tone={status.tone}>{status.label}</StatusBadge>;
    },
  },
  {
    id: "deliveries",
    header: "Delivery summary",
    cell: (message) => <DeliveryChips message={message} />,
  },
];

function Body({
  messages,
  error,
  onRetry,
  isStoreDown,
}: {
  messages: readonly MessageSummary[] | undefined;
  error: EngineError | null;
  onRetry: () => void;
  isStoreDown: boolean;
}) {
  if (isStoreDown || error?.code === "not_implemented") {
    return <p className={styles.muted}>The message store is not available yet.</p>;
  }
  if (messages === undefined) {
    return error === null ? (
      <LoadingState label="Loading messages…" rows={3} />
    ) : (
      <ErrorState title="The messages could not be loaded" error={error} onRetry={onRetry} />
    );
  }
  if (messages.length === 0) {
    return <p className={styles.muted}>This channel has not received a message yet.</p>;
  }
  return (
    <MiniTable
      label="Recent messages of this channel"
      columns={COLUMNS}
      rows={messages}
      getRowId={(message) => message.id}
    />
  );
}

/**
 * The newest messages of a message channel (operator-console.md section 4): when each was
 * received, its identifier, its state and what became of its deliveries, each opening the
 * record, and a way into the Messages list for the whole channel.
 */
export function RecentMessages({ channelId }: { channelId: string }) {
  const store = useSnapshot().data?.node.store;
  const isStoreDown = store?.state === "unavailable" || store?.state === "not_configured";
  const query = useEngineQuery(
    ["listMessages", "recent", channelId],
    (api) =>
      api.listMessages(
        toMessageQuery(
          { filters: { channels: [channelId] }, sort: DEFAULT_SORT, page: 1 },
          RECENT_MESSAGES,
          Date.now(),
        ),
      ),
    { refetchInterval: 5_000, enabled: !isStoreDown },
  );
  return (
    <Section
      title="Recent messages"
      actions={
        <div className={styles.actions}>
          <Button href={`/messages?channel=${encodeURIComponent(channelId)}&range=any`}>
            View all
          </Button>
        </div>
      }
    >
      <Body
        messages={query.data?.rows}
        error={query.error}
        isStoreDown={isStoreDown}
        onRetry={() => void query.refetch()}
      />
    </Section>
  );
}
