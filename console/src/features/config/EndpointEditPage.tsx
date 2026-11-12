import { useMemo, useState } from "react";
import { useForm, useWatch } from "react-hook-form";
import { Alert } from "@/components/Alert";
import { ChoiceInput } from "@/components/FormFields";
import { ENDPOINT_KINDS, endpointKindLabel } from "@/lib/status";
import { type BuiltDocument, ConfigEditor } from "./ConfigEditor";
import { endpointReferences, withEndpoint, withoutEndpoint } from "./configDocument";
import { EditNotFound } from "./EditNotFound";
import { ConnectionFields } from "./EndpointFields";
import {
  EMPTY_ENDPOINT_VALUES,
  type EndpointFormValues,
  endpointToValues,
  kindDefaults,
  valuesToEndpoint,
} from "./endpointForm";
import { GeneralFields } from "./GeneralFields";
import { type EditorProps, RecordEditPage } from "./RecordEditPage";
import { TestConnection, useConnectionTest } from "./TestConnection";

const KIND_OPTIONS = ENDPOINT_KINDS.map((kind) => ({ id: kind, label: endpointKindLabel(kind) }));

function Editor({ doc, id, onReload }: EditorProps) {
  // The record as it was when the page opened, so deleting it does not flash "not found".
  const [original] = useState(() => doc.endpoints.find((endpoint) => endpoint.id === id));
  const isNew = id === undefined;
  const form = useForm<EndpointFormValues>({
    defaultValues: original ? endpointToValues(original) : EMPTY_ENDPOINT_VALUES,
  });
  const kind = useWatch({ control: form.control, name: "kind" });
  const index = isNew ? doc.endpoints.length : doc.endpoints.findIndex((each) => each.id === id);
  const test = useConnectionTest(form, index);
  const takenIds = useMemo(() => new Set(doc.endpoints.map((endpoint) => endpoint.id)), [doc]);
  if (!isNew && original === undefined) {
    return <EditNotFound what="Endpoint" listLabel="Endpoints" listHref="/endpoints" />;
  }
  const entity = original
    ? {
        id: original.id,
        label: original.label,
        references: endpointReferences(doc, original.id),
        without: withoutEndpoint(doc, original.id),
      }
    : null;
  function build(values: EndpointFormValues): BuiltDocument {
    const endpoint = valuesToEndpoint(values);
    if (endpoint === null) return { document: doc, id: values.id };
    return { document: withEndpoint(doc, id, endpoint), id: endpoint.id };
  }
  return (
    <ConfigEditor
      noun="endpoint"
      doc={doc}
      form={form}
      entity={entity}
      entityIndex={index}
      build={build}
      onReload={onReload}
      testRejection={test.rejection}
    >
      <GeneralFields
        isNew={isNew}
        takenIds={takenIds}
        fixed={
          original && [
            { label: "Kind", value: endpointKindLabel(original.kind) },
            { label: "Id", value: original.id },
          ]
        }
        kindChoice={
          <ChoiceInput
            name="kind"
            label="Kind"
            options={KIND_OPTIONS}
            isRequired
            description="What this endpoint connects to. It cannot be changed later."
            onChange={(value) => {
              const next = ENDPOINT_KINDS.find((each) => each === value);
              if (next === undefined) return;
              const defaults = kindDefaults(next);
              form.setValue("port", defaults.port);
              form.setValue("pollIntervalSec", defaults.pollIntervalSec);
            }}
          />
        }
      />
      {kind === null ? (
        <Alert tone="info" compact>
          Choose a kind to see its connection settings.
        </Alert>
      ) : (
        <ConnectionFields kind={kind} />
      )}
      {kind !== null && <TestConnection test={test} kind={kind} />}
    </ConfigEditor>
  );
}

/**
 * Create (`/endpoints/new`) or edit (`/endpoints/:id/edit`) one endpoint on a local copy of
 * the configuration; the change is validated and applied as a whole.
 */
export function EndpointEditPage() {
  return <RecordEditPage what="endpoint" Editor={Editor} />;
}
