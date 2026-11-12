import { useMemo, useState } from "react";
import { useForm, useWatch } from "react-hook-form";
import { useSnapshot } from "@/api/hooks";
import type { ConfigDocument, EndpointState } from "@/api/types";
import { Alert } from "@/components/Alert";
import { ChoiceInput } from "@/components/FormFields";
import { channelKindLabel, endpointKindLabel, endpointStatus } from "@/lib/status";
import { BridgeFields } from "./BridgeFields";
import { type BuiltDocument, ConfigEditor } from "./ConfigEditor";
import type { EndpointOption } from "./ConnectorFields";
import {
  type ChannelFormValues,
  channelToValues,
  EMPTY_CHANNEL_VALUES,
  valuesToChannel,
} from "./channelForm";
import { slugify, withChannel, withoutChannel } from "./configDocument";
import { EditNotFound } from "./EditNotFound";
import { GeneralFields } from "./GeneralFields";
import { MessageFields } from "./MessageFields";
import { type EditorProps, RecordEditPage } from "./RecordEditPage";

const KIND_OPTIONS = (["bridge", "message"] as const).map((kind) => ({
  id: kind,
  label: channelKindLabel(kind),
}));

/** Endpoints to pick from, each with its kind and state so a wrong choice is easy to see. */
function useEndpointOptions(doc: ConfigDocument): EndpointOption[] {
  const snapshot = useSnapshot();
  const states = useMemo(
    () =>
      new Map<string, EndpointState>(snapshot.data?.endpoints.map((each) => [each.id, each.state])),
    [snapshot.data],
  );
  return useMemo(
    () =>
      doc.endpoints.map((endpoint) => {
        const state = states.get(endpoint.id);
        const status = endpoint.enabled
          ? state === undefined
            ? null
            : endpointStatus(state).label
          : "Disabled";
        // An id that is not the label's usual slug tells same-named endpoints apart.
        const detail = [
          endpointKindLabel(endpoint.kind),
          status,
          endpoint.id === slugify(endpoint.label) ? null : endpoint.id,
        ]
          .filter(Boolean)
          .join(" · ");
        return { id: endpoint.id, label: endpoint.label, description: detail, kind: endpoint.kind };
      }),
    [doc.endpoints, states],
  );
}

function Editor({ doc, id, onReload }: EditorProps) {
  // The record as it was when the page opened, so deleting it does not flash "not found".
  const [original] = useState(() => doc.channels.find((channel) => channel.id === id));
  const isNew = id === undefined;
  const form = useForm<ChannelFormValues>({
    defaultValues: original ? channelToValues(original) : EMPTY_CHANNEL_VALUES,
  });
  const kind = useWatch({ control: form.control, name: "kind" });
  const takenIds = useMemo(() => new Set(doc.channels.map((channel) => channel.id)), [doc]);
  const endpoints = useEndpointOptions(doc);
  if (!isNew && original === undefined) {
    return <EditNotFound what="Channel" listLabel="Channels" listHref="/channels" />;
  }
  const index = isNew ? doc.channels.length : doc.channels.findIndex((each) => each.id === id);
  const entity = original
    ? {
        id: original.id,
        label: original.label,
        references: [],
        without: withoutChannel(doc, original.id),
      }
    : null;
  function build(values: ChannelFormValues): BuiltDocument {
    const channel = valuesToChannel(values);
    if (channel === null) return { document: doc, id: values.id };
    return { document: withChannel(doc, id, channel), id: channel.id };
  }
  return (
    <ConfigEditor
      noun="channel"
      doc={doc}
      form={form}
      entity={entity}
      entityIndex={index}
      build={build}
      onReload={onReload}
    >
      <GeneralFields
        isNew={isNew}
        takenIds={takenIds}
        fixed={
          original && [
            { label: "Kind", value: channelKindLabel(original.kind) },
            { label: "Id", value: original.id },
          ]
        }
        kindChoice={
          <ChoiceInput
            name="kind"
            label="Kind"
            options={KIND_OPTIONS}
            isRequired
            description="A bridge passes bytes between two endpoints unchanged. A message channel stores, transforms and delivers messages. It cannot be changed later."
          />
        }
      />
      {kind === null && (
        <Alert tone="info" compact>
          Choose a kind to see its settings.
        </Alert>
      )}
      {kind === "bridge" && <BridgeFields endpoints={endpoints} />}
      {kind === "message" && (
        <MessageFields
          endpoints={endpoints}
          saved={
            original?.kind === "message"
              ? { channelId: original.id, destinationIds: original.destinations.map((d) => d.id) }
              : null
          }
        />
      )}
    </ConfigEditor>
  );
}

/**
 * Create (`/channels/new`) or edit (`/channels/:id/edit`) one bridge or message channel on a
 * local copy of the configuration; the change is validated and applied as a whole.
 */
export function ChannelEditPage() {
  return <RecordEditPage what="channel" Editor={Editor} />;
}
