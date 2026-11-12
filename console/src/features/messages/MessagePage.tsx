import { useState } from "react";
import { useLocation, useNavigate, useParams } from "react-router";
import { useEngineQuery, useSnapshot } from "@/api/hooks";
import type { Delivery, MessageDetail } from "@/api/types";
import { Button } from "@/components/Button";
import { EmptyState } from "@/components/EmptyState";
import { Page, PageHeader } from "@/components/Page";
import { QueryContent } from "@/components/QueryContent";
import { Section } from "@/components/Section";
import { StatusBadge } from "@/components/StatusBadge";
import { useLeaveOnIdle } from "@/lib/useLeaveOnIdle";
import { BodyViewer } from "./BodyViewer";
import { CloseDeliveryDialog } from "./CloseDeliveryDialog";
import { DeliveryCard } from "./DeliveryCard";
import { MessageFacts } from "./MessageFacts";
import { MessageNotices } from "./MessageNotices";
import styles from "./MessagePage.module.css";
import { SampleDataNotice, StoreNotAvailable, storeProblem } from "./MessageStoreNotice";
import { canResend, isInProgress, messageStateStatus, needsAttention } from "./messageWording";
import { listSearchOf } from "./navigation";
import { ResendDialog } from "./ResendDialog";
import { subjectOfDetail } from "./resendRules";

/** How often a record that is still changing is read again, in milliseconds. */
const LIVE_REFRESH_MS = 2000;

/** Whether the record can still change by itself: a transfer or a delivery is in progress. */
function isLive(detail: MessageDetail | undefined): boolean {
  if (detail === undefined) return false;
  const { state } = detail.summary;
  return (
    state === "receiving" ||
    state === "received" ||
    detail.deliveries.some((delivery) => isInProgress(delivery.state))
  );
}

/**
 * One message (CON-04, MSG-05, spec §6 Detail): its facts, original message, and each
 * destination's delivery with its attempts and transformed message, with Resend and Close. The
 * breadcrumb returns to the list as it was left. Bodies open only on request and nothing here
 * puts one in the address or the window title.
 */
export function MessagePage() {
  const { id = "" } = useParams();
  const listSearch = listSearchOf(useLocation().state);
  const snapshot = useSnapshot();
  const query = useEngineQuery(["getMessage", id], (api) => api.getMessage(id), {
    refetchInterval: (current) => (isLive(current.state.data) ? LIVE_REFRESH_MS : false),
  });
  const list = { label: "Messages", href: `/messages${listSearch}` };
  // Idle: back to the list, the record not kept (compliance P11).
  useLeaveOnIdle(list.href, "getMessage");
  const problem = storeProblem(query.error, snapshot.data?.node.store);
  if (query.error?.code === "not_found" && query.data === undefined) {
    return <NotFound listHref={list.href} current={`/messages/${encodeURIComponent(id)}`} />;
  }
  if (problem !== null && query.data === undefined) {
    return (
      <Page>
        <PageHeader
          title="Message"
          breadcrumbs={[list, { label: "Message", href: `/messages/${encodeURIComponent(id)}` }]}
        />
        <StoreNotAvailable problem={problem} />
      </Page>
    );
  }
  return (
    <Page>
      <SampleDataNotice />
      <QueryContent query={query} what="message" pageTitle="Message" loadingRows={6}>
        {(detail) => <MessageRecord detail={detail} list={list} />}
      </QueryContent>
    </Page>
  );
}

function NotFound({ listHref, current }: { listHref: string; current: string }) {
  const navigate = useNavigate();
  return (
    <Page>
      <PageHeader
        title="Message not found"
        breadcrumbs={[
          { label: "Messages", href: listHref },
          { label: "Not found", href: current },
        ]}
      />
      <EmptyState
        title="No stored message has this ID"
        description="It may have been removed by the channel's retention policy, or the address is wrong."
        action={<Button onPress={() => void navigate(listHref)}>Back to messages</Button>}
      />
    </Page>
  );
}

/** The identifier people use for a message: its control id, else its sample id, else its ID. */
function identifierOf(detail: MessageDetail): string {
  const { summary } = detail;
  return summary.controlId ?? summary.sampleId ?? summary.id;
}

type Dialog = { kind: "resend"; initial?: string[] } | { kind: "close"; delivery: Delivery };

function MessageRecord({
  detail,
  list,
}: {
  detail: MessageDetail;
  list: { label: string; href: string };
}) {
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const { summary } = detail;
  const identifier = identifierOf(detail);
  const status = messageStateStatus(summary.state);
  const resendable = canResend(summary.state);
  const unresolved = detail.deliveries.some((d) => needsAttention(d.state));
  const closeDialog = () => setDialog(null);
  return (
    <>
      <PageHeader
        title={identifier}
        breadcrumbs={[
          list,
          { label: identifier, href: `/messages/${encodeURIComponent(summary.id)}` },
        ]}
        status={<StatusBadge tone={status.tone}>{status.label}</StatusBadge>}
        actions={
          resendable && (
            <Button
              variant={unresolved ? "primary" : "secondary"}
              onPress={() => setDialog({ kind: "resend" })}
            >
              Resend…
            </Button>
          )
        }
      />
      <MessageNotices detail={detail} />
      <MessageFacts detail={detail} />
      <Section title="Original message">
        <BodyViewer
          messageId={summary.id}
          which={{ kind: "original" }}
          name="Original message"
          hasFields
        />
      </Section>
      <Section title="Deliveries">
        {detail.deliveries.length === 0 ? (
          <p>This message has no deliveries: nothing was queued for any destination.</p>
        ) : (
          <div className={styles.deliveries}>
            {detail.deliveries.map((delivery) => (
              <DeliveryCard
                key={delivery.id}
                delivery={delivery}
                canResend={resendable}
                onResend={(chosen) =>
                  setDialog({ kind: "resend", initial: [chosen.destinationId] })
                }
                onClose={(chosen) => setDialog({ kind: "close", delivery: chosen })}
              />
            ))}
          </div>
        )}
      </Section>
      {dialog?.kind === "resend" && (
        <ResendDialog
          subject={subjectOfDetail(detail)}
          initialDestinations={dialog.initial}
          onClose={closeDialog}
        />
      )}
      {dialog?.kind === "close" && (
        <CloseDeliveryDialog delivery={dialog.delivery} onClose={closeDialog} />
      )}
    </>
  );
}
