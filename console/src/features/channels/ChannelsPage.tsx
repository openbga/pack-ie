import { useMemo } from "react";
import { useNavigate } from "react-router";
import { useSnapshot } from "@/api/hooks";
import type { ChannelKind, ChannelSnapshot, Snapshot } from "@/api/types";
import { Button } from "@/components/Button";
import { Checkbox } from "@/components/Checkbox";
import { Select } from "@/components/Select";
import { channelNeedsAttention, compareChannels } from "../shared-operate/channelRules";
import { Directory } from "../shared-operate/Directory";
import { useChoiceParam, useClearParams, useFlagParam } from "../shared-operate/searchParam";
import { useChannelActions } from "../shared-operate/useChannelActions";
import {
  CHANNEL_KIND_OPTIONS,
  CHANNEL_STATE_OPTIONS,
  CHANNEL_STATES,
  type ChannelStateKey,
  channelColumnsFor,
  channelStateKey,
} from "./channelColumns";

const KINDS = ["bridge", "message"] as const satisfies readonly ChannelKind[];
const STATES = CHANNEL_STATES.map((choice) => choice.id);
const FILTER_PARAMS = ["kind", "state", "attention"] as const;
const FIND_FIELDS = (channel: ChannelSnapshot) => [
  channel.label,
  ...channel.endpoints.map((endpoint) => endpoint.label),
];

/** Failing first, so the channel that needs someone is the first row. */
const channelsOf = (snapshot: Snapshot): readonly ChannelSnapshot[] =>
  [...snapshot.channels].sort(compareChannels);

/**
 * Channels directory (CON-03): every channel with its state, traffic, queue pressure and last
 * error, failing ones first. One click opens the channel; the row menu pauses, resumes,
 * restarts or edits it. Live: the list follows the engine's snapshot.
 */
export function ChannelsPage() {
  const snapshot = useSnapshot();
  const navigate = useNavigate();
  const actions = useChannelActions();
  const [kind, setKind] = useChoiceParam("kind", KINDS);
  const [state, setState] = useChoiceParam<ChannelStateKey>("state", STATES);
  const [attention, setAttention] = useFlagParam("attention");
  const clear = useClearParams(FILTER_PARAMS);
  const keep = useMemo(
    () => (channel: ChannelSnapshot) =>
      (kind === null || channel.kind === kind) &&
      (state === null || channelStateKey(channel) === state) &&
      (!attention || channelNeedsAttention(channel)),
    [kind, state, attention],
  );
  const addChannel = (
    <Button href="/channels/new" variant="primary">
      Add channel
    </Button>
  );
  return (
    <Directory
      title="Channels"
      noun={{ many: "channels", one: "channel" }}
      headerAction={addChannel}
      query={snapshot}
      rowsOf={channelsOf}
      keep={keep}
      isFiltered={kind !== null || state !== null || attention}
      onClearFilters={clear}
      find={{ placeholder: "Channel or endpoint name", fields: FIND_FIELDS }}
      filters={
        <>
          <Select
            label="Kind"
            layout="inline"
            name="kind"
            options={CHANNEL_KIND_OPTIONS}
            value={kind ?? "all"}
            onChange={(value) => setKind(KINDS.find((each) => each === value) ?? null)}
          />
          <Select
            label="State"
            layout="inline"
            name="state"
            options={CHANNEL_STATE_OPTIONS}
            value={state ?? "all"}
            onChange={(value) => setState(STATES.find((each) => each === value) ?? null)}
          />
          <Checkbox isSelected={attention} onChange={setAttention}>
            Needs attention only
          </Checkbox>
        </>
      }
      empty={{
        title: "No channels yet",
        description:
          "A channel connects an instrument or system to another: a bridge passes bytes between two endpoints, a message channel stores and delivers messages.",
        action: addChannel,
      }}
      columns={channelColumnsFor}
      getRowId={(channel) => channel.id}
      onRowAction={(channel) => void navigate(`/channels/${channel.id}`)}
      rowActions={{
        label: (channel) => channel.label,
        hasItems: () => true,
        items: actions.items,
      }}
    >
      {actions.dialog}
    </Directory>
  );
}
