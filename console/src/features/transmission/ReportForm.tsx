import { Button } from "@/components/Button";
import { type RecordOption, RecordPicker } from "@/components/RecordPicker";
import { TimeRangeFields } from "@/components/TimeRangeFields";
import { without, withValue } from "@/lib/optionalKeys";
import layout from "@/styles/layout.module.css";
import styles from "./Transmission.module.css";
import { PERIOD_CHOICES, periodProblem, type TransmissionView } from "./transmissionParams";

interface ReportFormProps {
  view: TransmissionView;
  onChange: (next: TransmissionView) => void;
  /** The message channels to choose from. */
  channels: readonly RecordOption[];
  /** The channels are still being read. */
  channelsPending: boolean;
  isRunning: boolean;
  onRun: () => void;
}

/**
 * Channel and period. Changing them never runs the report: the Run report button does, and
 * says it is recorded in the audit log.
 */
export function ReportForm({
  view,
  onChange,
  channels,
  channelsPending,
  isRunning,
  onRun,
}: ReportFormProps) {
  const problem = view.channel === undefined ? "Choose a channel." : periodProblem(view);
  return (
    <form
      className={layout.stack}
      onSubmit={(event) => {
        event.preventDefault();
        if (problem === null && !isRunning) onRun();
      }}
    >
      <div className={styles.form}>
        <div className={styles.channel}>
          <RecordPicker
            label="Channel"
            name="channel"
            options={channels}
            value={view.channel ?? null}
            onChange={(channel) =>
              onChange(
                channel === null
                  ? without(view, "channel", "destination")
                  : { ...without(view, "destination"), channel },
              )
            }
            isDisabled={channelsPending && channels.length === 0}
            description={
              channelsPending && channels.length === 0
                ? "Loading channels…"
                : channels.length === 0
                  ? "No message channels are configured."
                  : "Message channels only: a bridge passes bytes and keeps no deliveries."
            }
          />
        </div>
        <TimeRangeFields
          label="Period"
          ranges={PERIOD_CHOICES}
          range={view.range}
          from={view.since}
          to={view.until}
          onRange={(range) => onChange({ ...without(view, "since", "until"), range })}
          onFrom={(since) => onChange(withValue(view, "since", since))}
          onTo={(until) => onChange(withValue(view, "until", until))}
        />
        <Button type="submit" variant="primary" isPending={isRunning} isDisabled={problem !== null}>
          Run report
        </Button>
      </div>
      <p className={layout.hint}>
        {problem === null ? "" : `${problem} `}Running a report is recorded in the audit log with
        your account.
      </p>
    </form>
  );
}
