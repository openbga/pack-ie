import { useMemo } from "react";
import { useNavigate } from "react-router";
import { useSnapshot } from "@/api/hooks";
import type { EndpointKind, EndpointState, Snapshot } from "@/api/types";
import { Button } from "@/components/Button";
import { Select } from "@/components/Select";
import { ENDPOINT_KINDS, endpointUrgency } from "@/lib/status";
import { Directory } from "../shared-operate/Directory";
import { useChoiceParam, useClearParams } from "../shared-operate/searchParam";
import {
  ENDPOINT_KIND_OPTIONS,
  ENDPOINT_STATE_OPTIONS,
  ENDPOINT_STATES,
  type EndpointRow,
  endpointColumnsFor,
} from "./endpointColumns";

const FILTER_PARAMS = ["kind", "state"] as const;
const FIND_FIELDS = (endpoint: EndpointRow) => [
  endpoint.label,
  endpoint.summary,
  ...endpoint.channelLabels,
];

/** Failing endpoints first, each with the labels of the channels that use it. */
function endpointsOf(snapshot: Snapshot): readonly EndpointRow[] {
  const channelLabel = new Map(snapshot.channels.map((channel) => [channel.id, channel.label]));
  return snapshot.endpoints
    .map((endpoint) => ({
      ...endpoint,
      channelLabels: endpoint.channelIds.map((id) => channelLabel.get(id) ?? id),
    }))
    .sort(
      (a, b) =>
        endpointUrgency(b.state) - endpointUrgency(a.state) || a.label.localeCompare(b.label),
    );
}

/**
 * Endpoints directory (EPT-06): every endpoint with its state and for how long, traffic, the
 * channels that use it and its last error, failing ones first. Live, from the snapshot.
 */
export function EndpointsPage() {
  const snapshot = useSnapshot();
  const navigate = useNavigate();
  const [kind, setKind] = useChoiceParam<EndpointKind>("kind", ENDPOINT_KINDS);
  const [state, setState] = useChoiceParam<EndpointState>("state", ENDPOINT_STATES);
  const clear = useClearParams(FILTER_PARAMS);
  const keep = useMemo(
    () => (endpoint: EndpointRow) =>
      (kind === null || endpoint.kind === kind) && (state === null || endpoint.state === state),
    [kind, state],
  );
  const addEndpoint = (
    <Button href="/endpoints/new" variant="primary">
      Add endpoint
    </Button>
  );
  return (
    <Directory
      title="Endpoints"
      noun={{ many: "endpoints", one: "endpoint" }}
      headerAction={addEndpoint}
      query={snapshot}
      rowsOf={endpointsOf}
      keep={keep}
      isFiltered={kind !== null || state !== null}
      onClearFilters={clear}
      find={{ placeholder: "Name, address or channel", fields: FIND_FIELDS }}
      filters={
        <>
          <Select
            label="Kind"
            layout="inline"
            name="kind"
            options={ENDPOINT_KIND_OPTIONS}
            value={kind ?? "all"}
            onChange={(value) => setKind(ENDPOINT_KINDS.find((each) => each === value) ?? null)}
          />
          <Select
            label="State"
            layout="inline"
            name="state"
            options={ENDPOINT_STATE_OPTIONS}
            value={state ?? "all"}
            onChange={(value) => setState(ENDPOINT_STATES.find((each) => each === value) ?? null)}
          />
        </>
      }
      empty={{
        title: "No endpoints yet",
        description:
          "An endpoint is one side of a connection: a serial port, a TCP client or listener, a folder or an SFTP server. Channels connect endpoints.",
        action: addEndpoint,
      }}
      columns={endpointColumnsFor}
      getRowId={(endpoint) => endpoint.id}
      onRowAction={(endpoint) => void navigate(`/endpoints/${endpoint.id}`)}
      rowActions={{
        label: (endpoint) => endpoint.label,
        hasItems: () => true,
        items: (endpoint) => [
          {
            id: "edit",
            label: "Edit",
            onAction: () => void navigate(`/endpoints/${endpoint.id}/edit`),
          },
        ],
      }}
    />
  );
}
