import { useMemo, useState } from "react";
import { useForm } from "react-hook-form";
import { useEngineQuery, useSnapshot } from "@/api/hooks";
import type { ConfigDocument, MessageChannelConfig, StoreStatus } from "@/api/types";
import { Alert } from "@/components/Alert";
import { Button } from "@/components/Button";
import { DataGrid, type DataGridColumn, type GridRowActions } from "@/components/DataGrid";
import { Facts } from "@/components/Facts";
import { FieldGrid } from "@/components/Form";
import { FormDialog } from "@/components/FormDialog";
import { TextInput } from "@/components/FormFields";
import { QueryContent } from "@/components/QueryContent";
import { Section } from "@/components/Section";
import { useToast } from "@/components/Toast";
import { appliedToast, useApplyDocument } from "@/features/config/applyDocument";
import { changeNoteValidator } from "@/features/config/changeNote";
import { withChannel } from "@/features/config/configDocument";
import { optionalWholeNumber } from "@/features/config/validators";
import { formatCount } from "@/lib/format";
import { storeSummary } from "@/lib/store";
import styles from "@/styles/layout.module.css";

const COLUMNS: DataGridColumn<MessageChannelConfig>[] = [
  {
    id: "channel",
    header: "Channel",
    identifier: true,
    cell: (row) => row.label,
    sortValue: (row) => row.label,
    defaultSort: "asc",
  },
  {
    id: "days",
    header: "Keep for",
    align: "end",
    width: 160,
    cell: (row) => (row.retention.days === null ? "No age limit" : `${row.retention.days} days`),
    sortValue: (row) => row.retention.days ?? Number.POSITIVE_INFINITY,
  },
  {
    id: "maxMessages",
    header: "Keep at most",
    align: "end",
    width: 200,
    cell: (row) =>
      row.retention.maxMessages === null
        ? "No count limit"
        : `${formatCount(row.retention.maxMessages)} messages`,
    sortValue: (row) => row.retention.maxMessages ?? Number.POSITIVE_INFINITY,
  },
];

interface RetentionValues {
  retention: { days: string; maxMessages: string };
  changeNote: string;
}

function RetentionDialog({
  doc,
  channel,
  onClose,
  onReload,
}: {
  doc: ConfigDocument;
  channel: MessageChannelConfig;
  onClose: () => void;
  onReload: () => void;
}) {
  const toast = useToast();
  const apply = useApplyDocument();
  const index = doc.channels.findIndex((each) => each.id === channel.id);
  const form = useForm<RetentionValues>({
    defaultValues: {
      retention: {
        days: channel.retention.days === null ? "" : String(channel.retention.days),
        maxMessages:
          channel.retention.maxMessages === null ? "" : String(channel.retention.maxMessages),
      },
      changeNote: "",
    },
  });
  function save({ retention, changeNote }: RetentionValues) {
    const next: MessageChannelConfig = {
      ...channel,
      retention: {
        days: retention.days.trim() === "" ? null : Number(retention.days),
        maxMessages: retention.maxMessages.trim() === "" ? null : Number(retention.maxMessages),
      },
    };
    const document = withChannel(doc, channel.id, next);
    apply.mutate(
      { document, note: changeNote },
      {
        onSuccess: (result) => {
          toast.show(appliedToast(result, document.channels));
          onClose();
        },
      },
    );
  }
  return (
    <FormDialog
      title={`Edit retention of ${channel.label}`}
      description="Saving applies a new configuration version; the channel is restarted only if the engine needs it."
      submitLabel="Save retention"
      form={form}
      onSubmit={(event) => void form.handleSubmit(save)(event)}
      command={apply}
      errorTitle="The retention was not saved"
      fieldPaths={{ [`channels.${index}`]: "" }}
      onClose={onClose}
      extraActions={
        apply.error?.code === "conflict" ? (
          <Button
            onPress={() => {
              apply.reset();
              onReload();
            }}
          >
            Reload configuration
          </Button>
        ) : undefined
      }
    >
      <Alert tone="info" compact>
        Purging only removes messages whose deliveries are all finished: acknowledged or closed.
        Undelivered and unresolved messages are never purged. Leave a field empty for no limit.
      </Alert>
      <FieldGrid>
        <TextInput name="retention.days" label="Keep for (days)" validate={optionalWholeNumber} />
        <TextInput
          name="retention.maxMessages"
          label="Keep at most (messages)"
          validate={optionalWholeNumber}
        />
        <TextInput
          name="changeNote"
          label="Change note"
          isRequired
          validate={changeNoteValidator}
          description="Why you are changing retention, 1 to 500 characters. Recorded with the version and in the audit log."
        />
      </FieldGrid>
    </FormDialog>
  );
}

function storeFacts(store: StoreStatus | null) {
  const summary = storeSummary(store);
  return [
    { label: "Message store", value: summary.state },
    { label: "Messages stored", value: summary.messages },
    { label: "Database size", value: summary.size },
    { label: "Disk free", value: summary.disk },
  ];
}

function StoreStatusSection() {
  const snapshot = useSnapshot();
  return (
    <Section title="Message store">
      <QueryContent query={snapshot} what="store status">
        {({ node }) => <Facts layout="compact" items={storeFacts(node.store)} />}
      </QueryContent>
    </Section>
  );
}

function RetentionTable({ doc, onReload }: { doc: ConfigDocument; onReload: () => void }) {
  const [editing, setEditing] = useState<string | null>(null);
  const rows = useMemo(
    () => doc.channels.filter((channel) => channel.kind === "message"),
    [doc.channels],
  );
  const channel = rows.find((row) => row.id === editing);
  return (
    <>
      <p className={styles.hint}>
        Old messages are purged by these limits, but never a message that is undelivered or
        unresolved.
      </p>
      <DataGrid
        label="Retention by channel"
        columns={COLUMNS}
        rows={rows}
        getRowId={(row) => row.id}
        onRowAction={(row) => setEditing(row.id)}
        rowActions={rowActions(setEditing)}
      />
      {channel && (
        <RetentionDialog
          doc={doc}
          channel={channel}
          onClose={() => setEditing(null)}
          onReload={onReload}
        />
      )}
    </>
  );
}

function rowActions(edit: (id: string) => void): GridRowActions<MessageChannelConfig> {
  return {
    label: (row) => row.label,
    hasItems: () => true,
    items: (row) => [{ id: "edit", label: "Edit retention…", onAction: () => edit(row.id) }],
  };
}

/** Per-channel retention (MSG-08) and the state of the message store. */
export function RetentionTab() {
  const config = useEngineQuery(["getConfig"], (api) => api.getConfig());
  return (
    <div className={styles.stack}>
      <StoreStatusSection />
      <Section title="Retention by channel">
        <QueryContent
          query={config}
          what="retention settings"
          loadingRows={3}
          empty={{
            when: (doc) => doc.channels.every((channel) => channel.kind !== "message"),
            title: "No message channels yet",
            description:
              "Retention applies to message channels, which store the messages they handle.",
          }}
        >
          {(doc) => <RetentionTable doc={doc} onReload={() => void config.refetch()} />}
        </QueryContent>
      </Section>
    </div>
  );
}
