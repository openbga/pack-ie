import { type ReactNode, useState } from "react";
import { useNavigate } from "react-router";
import { useEngineMutation, useSnapshot } from "@/api/hooks";
import type { ChannelAction, ChannelKind, ChannelSnapshot } from "@/api/types";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import type { ActionItem } from "@/components/RowActions";
import { useToast } from "@/components/Toast";
import { toUserMessage } from "@/lib/errors";
import { allowedChannelActions } from "./channelRules";

type ChannelRef = Pick<
  ChannelSnapshot,
  "id" | "label" | "kind" | "state" | "stopReason" | "allowedActions"
>;

export const CHANNEL_ACTION_LABELS: Record<ChannelAction, string> = {
  pause: "Pause",
  resume: "Resume",
  restart: "Restart",
};

/** What the operator is told before the command runs (operator-console.md section 5). */
const CONSEQUENCE: Record<"pause" | "restart", (kind: ChannelKind) => string> = {
  pause: (kind) =>
    kind === "bridge"
      ? "The session closes: both endpoints are released, so the instrument or system on either side sees the connection drop, and a listening side refuses new connections. No retry is scheduled. The channel stays paused until you resume it."
      : "The sender will see its connection refused. Messages already accepted stay queued and are kept in the store; their deliveries wait and continue after you resume. No retry is scheduled.",
  restart: (kind) =>
    kind === "bridge"
      ? "The session closes and both endpoints are reopened. A transfer in progress is interrupted, and the instrument may need to send it again. The counters of this instance reset."
      : "The session closes and the endpoints are reopened. A message being received or delivered right now is interrupted; stored messages are kept and unfinished deliveries are retried. The counters of this instance reset.",
};

const CONFIRM_LABEL = { pause: "Pause channel", restart: "Restart channel" } as const;

const VERB: Record<ChannelAction, string> = {
  pause: "pause",
  resume: "resume",
  restart: "restart",
};

interface Command {
  channel: ChannelRef;
  action: ChannelAction;
}

/** The outcome in the operator's words; the version is the one the new instance starts from. */
function outcomeTitle(action: ChannelAction, configVersion: number | undefined): string {
  if (action === "pause") return "Channel paused. No retry is scheduled.";
  const from =
    configVersion === undefined
      ? "the active configuration"
      : `configuration version ${configVersion}`;
  return action === "resume" ? `Starting from ${from}.` : `Restarting from ${from}.`;
}

/**
 * Pause, resume and restart of a channel. Pause and restart ask first and say what will
 * happen; resume runs at once. The outcome is a toast in the spec's words, and a refusal is
 * shown as the engine words it. `items` is the menu for a grid row (the actions allowed in
 * the channel's state, then Edit); `ask` starts one action; render `dialog` once on the screen.
 */
export function useChannelActions(): {
  items: (channel: ChannelRef) => ActionItem[];
  ask: (channel: ChannelRef, action: ChannelAction) => void;
  dialog: ReactNode;
} {
  const navigate = useNavigate();
  const toast = useToast();
  const snapshot = useSnapshot();
  const [pending, setPending] = useState<Command | null>(null);
  const run = useEngineMutation<Command>(
    (api, { channel, action }) => {
      if (action === "pause") return api.pauseChannel(channel.id);
      if (action === "resume") return api.resumeChannel(channel.id);
      return api.restartChannel(channel.id);
    },
    { invalidates: ["getChannel"] },
  );

  function execute(command: Command) {
    const { channel, action } = command;
    run.mutate(command, {
      onSuccess: () => {
        toast.show({
          tone: "success",
          title: outcomeTitle(action, snapshot.data?.node.configVersion),
          description: channel.label,
        });
        setPending(null);
      },
      onError: (error) => {
        toast.show({
          tone: "danger",
          title: `Could not ${VERB[action]} ${channel.label}`,
          description: toUserMessage(error),
        });
        setPending(null);
      },
    });
  }

  function ask(channel: ChannelRef, action: ChannelAction) {
    if (action === "resume") execute({ channel, action });
    else setPending({ channel, action });
  }

  const items = (channel: ChannelRef): ActionItem[] => [
    ...allowedChannelActions(channel).map((action) => ({
      id: action,
      label: CHANNEL_ACTION_LABELS[action],
      onAction: () => ask(channel, action),
    })),
    { id: "edit", label: "Edit", onAction: () => void navigate(`/channels/${channel.id}/edit`) },
  ];

  const dialog = pending && pending.action !== "resume" && (
    <ConfirmDialog
      title={`${CHANNEL_ACTION_LABELS[pending.action]} ${pending.channel.label}?`}
      isOpen
      onOpenChange={(open) => {
        if (!open && !run.isPending) setPending(null);
      }}
      confirmLabel={CONFIRM_LABEL[pending.action]}
      variant="danger"
      isPending={run.isPending}
      onConfirm={() => execute(pending)}
    >
      <p>{CONSEQUENCE[pending.action](pending.channel.kind)}</p>
    </ConfirmDialog>
  );

  return { items, ask, dialog };
}
