import { useMemo } from "react";
import { useEngineMutation, useSnapshot } from "@/api/hooks";
import type { ChannelSnapshot, TransmissionQuery } from "@/api/types";
import { EmptyState } from "@/components/EmptyState";
import { ErrorState } from "@/components/ErrorState";
import { LoadingState } from "@/components/LoadingState";
import { Page, PageHeader } from "@/components/Page";
import type { RecordOption } from "@/components/RecordPicker";
import { Section } from "@/components/Section";
import { protocolLabel } from "@/lib/status";
import { ReportForm } from "./ReportForm";
import { ReportResult } from "./ReportResult";
import { toQuery } from "./transmissionParams";
import { useTransmissionView } from "./useTransmissionView";

/**
 * The message channels to choose from, by label. A channel named by the address that the
 * configuration does not list stays choosable: the engine reports on stored messages whether
 * or not the channel is still configured, and answers `not_found` for an id it never saw.
 */
function channelOptions(
  channels: readonly ChannelSnapshot[] | undefined,
  chosen: string | undefined,
): RecordOption[] {
  const options = (channels ?? [])
    .filter((channel) => channel.kind === "message")
    .map((channel) => ({
      id: channel.id,
      label: channel.label,
      ...(channel.protocol === null ? {} : { description: protocolLabel(channel.protocol) }),
    }))
    .sort((a, b) => a.label.localeCompare(b.label));
  if (chosen !== undefined && channels !== undefined && !options.some((o) => o.id === chosen)) {
    options.push({ id: chosen, label: chosen, description: "not in the configuration" });
  }
  return options;
}

/**
 * Transmission report (docs/spec/operator-console.md section 6; CAP GEN.48500): for a channel
 * and period, what each destination confirmed for the messages received, with the hashes to
 * compare against the receiving system. Running it is an explicit action, recorded in the audit
 * log; the form is kept in the URL.
 */
export function TransmissionPage() {
  const { view, setView } = useTransmissionView();
  const snapshot = useSnapshot();
  const run = useEngineMutation((api, query: TransmissionQuery) => api.verifyTransmission(query), {
    invalidates: ["getAuditEvents"],
  });
  const channels = useMemo(
    () => channelOptions(snapshot.data?.channels, view.channel),
    [snapshot.data?.channels, view.channel],
  );
  const labelOf = (id: string) => channels.find((channel) => channel.id === id)?.label ?? id;
  function start() {
    const query = toQuery(view, Date.now());
    if (query !== null) run.mutate(query);
  }
  return (
    <Page>
      <PageHeader
        title="Transmission report"
        description="What each destination confirmed for the messages a channel received in a period, to compare with what the receiving system shows."
      />
      <Section title="Channel and period">
        <ReportForm
          view={view}
          onChange={setView}
          channels={channels}
          channelsPending={snapshot.isPending}
          isRunning={run.isPending}
          onRun={start}
        />
      </Section>
      {run.isPending && <LoadingState label="Running the report…" rows={6} />}
      {run.isError && (
        <ErrorState
          title="The report could not be run"
          error={run.error}
          onRetry={() => run.variables && run.mutate(run.variables)}
          isRetrying={run.isPending}
        />
      )}
      {run.isIdle && (
        <EmptyState
          title="No report yet"
          description="Choose a channel and a period, then select Run report."
        />
      )}
      {run.isSuccess && (
        <ReportResult
          report={run.data}
          channelLabel={labelOf(run.data.channelId)}
          destination={view.destination}
          onDestination={(destination) => {
            const { destination: _removed, ...rest } = view;
            setView(destination === undefined ? rest : { ...rest, destination });
          }}
        />
      )}
    </Page>
  );
}
