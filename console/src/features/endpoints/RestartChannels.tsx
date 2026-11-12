import { useState } from "react";
import { useEngineMutation } from "@/api/hooks";
import type { ChannelSnapshot } from "@/api/types";
import { Button } from "@/components/Button";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { useToast } from "@/components/Toast";
import { toEngineError, toUserMessage } from "@/lib/errors";
import { allowedChannelActions } from "../shared-operate/channelRules";

type UsingChannel = Pick<
  ChannelSnapshot,
  "id" | "label" | "kind" | "state" | "stopReason" | "allowedActions"
>;

interface Outcome {
  channel: UsingChannel;
  /** The engine's words when it refused or failed; null when it accepted. */
  problem: string | null;
}

/**
 * "Restart the channels that use it": a shortcut for `restart_channel` on each channel that
 * can be restarted (operator-console.md section 4). The confirmation lists the channels that
 * will restart and those that will be left alone (paused, disabled or stopped), and says the
 * sessions close and counters reset. Refusals are reported per channel in the engine's words.
 */
export function RestartChannels({
  endpointLabel,
  channels,
}: {
  endpointLabel: string;
  channels: readonly UsingChannel[];
}) {
  const toast = useToast();
  const [isOpen, setIsOpen] = useState(false);
  const restartable = channels.filter((channel) =>
    allowedChannelActions(channel).includes("restart"),
  );
  const skipped = channels.filter((channel) => !restartable.includes(channel));
  const run = useEngineMutation<readonly UsingChannel[], Outcome[]>(
    async (api, targets) =>
      Promise.all(
        targets.map(async (channel): Promise<Outcome> => {
          try {
            await api.restartChannel(channel.id);
            return { channel, problem: null };
          } catch (error) {
            return { channel, problem: toUserMessage(toEngineError(error)) };
          }
        }),
      ),
    { invalidates: ["getChannel", "getEndpoint"] },
  );

  function confirm() {
    run.mutate(restartable, {
      onSuccess: (outcomes) => {
        const failed = outcomes.filter((outcome) => outcome.problem !== null);
        const done = outcomes.length - failed.length;
        if (failed.length === 0) {
          toast.show({
            tone: "success",
            title: `Restarting ${done} ${done === 1 ? "channel" : "channels"}.`,
            description: outcomes.map((outcome) => outcome.channel.label).join(", "),
          });
        } else {
          toast.show({
            tone: "danger",
            title: `${failed.length} of ${outcomes.length} could not be restarted`,
            description: failed
              .map((outcome) => `${outcome.channel.label}: ${outcome.problem}`)
              .join(" "),
          });
        }
        setIsOpen(false);
      },
    });
  }

  if (restartable.length === 0) return null;
  return (
    <>
      <Button onPress={() => setIsOpen(true)}>Restart the channels that use it</Button>
      <ConfirmDialog
        title={`Restart the channels that use ${endpointLabel}?`}
        isOpen={isOpen}
        onOpenChange={(open) => {
          if (!run.isPending) setIsOpen(open);
        }}
        confirmLabel={`Restart ${restartable.length} ${restartable.length === 1 ? "channel" : "channels"}`}
        isPending={run.isPending}
        onConfirm={confirm}
      >
        <p>
          Each session closes and its endpoints are reopened. A transfer in progress is interrupted,
          and the counters of each restarted instance reset.
        </p>
        <p>Will restart:</p>
        <ul aria-label="Channels to restart">
          {restartable.map((channel) => (
            <li key={channel.id}>{channel.label}</li>
          ))}
        </ul>
        {skipped.length > 0 && (
          <>
            <p>Left as they are (paused, disabled or stopped):</p>
            <ul aria-label="Channels left alone">
              {skipped.map((channel) => (
                <li key={channel.id}>{channel.label}</li>
              ))}
            </ul>
          </>
        )}
      </ConfirmDialog>
    </>
  );
}
