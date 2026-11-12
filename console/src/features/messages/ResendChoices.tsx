import { Link, Radio, RadioGroup } from "react-aria-components";
import { Controller } from "react-hook-form";
import { useEngineQuery } from "@/api/hooks";
import type { ResendMode } from "@/api/types";
import { Alert } from "@/components/Alert";
import { Checkbox } from "@/components/Checkbox";
import { CheckboxGroup } from "@/components/CheckboxGroup";
import { Timestamp } from "@/components/Timestamp";
import { deliveryLabel, newMessageOnly } from "./messageWording";
import { ReasonField } from "./ReasonField";
import styles from "./ResendChoices.module.css";
import { blockedDestinations, needsRiskConfirmation, type ResendSubject } from "./resendRules";

/** One sentence per mode (operator-console.md §6). */
const MODES: Record<ResendMode, { label: string; sentence: string }> = {
  same_message: { label: "Same message", sentence: "Try again with the same output" },
  new_message: {
    label: "New message",
    sentence: "Process the original again with the current configuration",
  },
};

/** The form's values, which the choices read and write. */
export interface ResendValues {
  destinations: string[];
  mode: ResendMode;
  confirm: boolean;
  confirmSuperseded: boolean;
  reason: string;
}

interface ResendChoicesProps {
  subject: ResendSubject;
  modes: readonly ResendMode[];
  risk: boolean;
  /** The message is superseded: the newer message when it is known. */
  superseded: { newerId: string | null } | null;
}

/** What the operator decides: destinations, mode, the confirmations that apply, and the reason. */
export function ResendChoices({ subject, modes, risk, superseded }: ResendChoicesProps) {
  const hasDeliveries = subject.destinations.length > 0;
  return (
    <>
      {superseded && <SupersededConfirmation newerId={superseded.newerId} />}
      {hasDeliveries ? (
        <DestinationChoice subject={subject} />
      ) : (
        <p>
          This message has no deliveries. The resend goes to every enabled destination of{" "}
          {subject.channelLabel} that accepts it under the current configuration.
        </p>
      )}
      <ModeChoice modes={modes} subject={subject} />
      {risk && <RiskConfirmation subject={subject} />}
      <ReasonField name="reason" />
    </>
  );
}

/**
 * The warning that a newer revision exists, naming it with the time it was received, and the
 * confirmation that sends `confirmSuperseded`. Shown whenever the engine would refuse.
 */
function SupersededConfirmation({ newerId }: { newerId: string | null }) {
  const newer = useEngineQuery(
    ["getMessage", newerId ?? ""],
    (api) => api.getMessage(newerId ?? ""),
    {
      enabled: newerId !== null,
    },
  );
  const received = newer.data?.summary.receivedAt;
  return (
    <Controller<ResendValues, "confirmSuperseded">
      name="confirmSuperseded"
      rules={{ validate: (value) => value || "Confirm to resend this older version." }}
      render={({ field, fieldState }) => (
        <div className={styles.confirmation}>
          <Alert tone="warning" title="A newer revision of this message exists" compact>
            <p>
              A newer revision exists
              {received !== undefined && (
                <>
                  , received <Timestamp value={received} seconds />
                </>
              )}
              . Resending this older version may reissue a corrected result.
            </p>
            {newerId !== null && (
              <p>
                Newer message:{" "}
                <Link href={`/messages/${encodeURIComponent(newerId)}`}>{newerId}</Link>
              </p>
            )}
          </Alert>
          <Checkbox isSelected={field.value} onChange={field.onChange}>
            I understand this may reissue a corrected result; resend this older version anyway
          </Checkbox>
          {fieldState.error && <p className={styles.error}>{fieldState.error.message}</p>}
        </div>
      )}
    />
  );
}

function DestinationChoice({ subject }: { subject: ResendSubject }) {
  return (
    <Controller<ResendValues, "destinations">
      name="destinations"
      rules={{
        validate: (value, values) => {
          if (value.length === 0) return "Choose at least one destination.";
          return blockedDestinations(subject, value, values.mode).length === 0
            ? true
            : "A delivery that is queued or sending cannot be resent as the same message. Leave it out, or choose a new message.";
        },
      }}
      render={({ field, fieldState }) => (
        <CheckboxGroup
          legend="Destinations"
          isRequired
          options={subject.destinations.map((d) => ({
            id: d.id,
            label: d.label,
            details: [deliveryLabel(d.state)],
          }))}
          value={field.value}
          onChange={field.onChange}
          errorMessage={fieldState.error?.message}
        />
      )}
    />
  );
}

function ModeChoice({ modes, subject }: { modes: readonly ResendMode[]; subject: ResendSubject }) {
  const only = modes.length === 1 ? modes[0] : undefined;
  if (only !== undefined) {
    return (
      <div>
        <p className={styles.legend}>Mode</p>
        <p className={styles.fixed}>
          <strong>{MODES[only].label}.</strong> {MODES[only].sentence}.
        </p>
        <p className={styles.explanation}>
          {newMessageOnly(subject.state)
            ? "This message was not routed, so it can only be processed again as a new message."
            : "No delivery exists to try again, so the message is processed again from its original."}{" "}
          The original and its record stay as they are.
        </p>
      </div>
    );
  }
  return (
    <Controller<ResendValues, "mode">
      name="mode"
      render={({ field }) => (
        <RadioGroup
          value={field.value}
          onChange={(next) => field.onChange(next)}
          className={styles.modes}
        >
          <span className={styles.legend}>Mode</span>
          {modes.map((id) => (
            <Radio key={id} value={id} className={styles.mode}>
              <span className={styles.modeLabel}>
                <span className={styles.indicator} aria-hidden="true" />
                {MODES[id].label}
              </span>
              <span className={styles.explanation}>{MODES[id].sentence}</span>
            </Radio>
          ))}
        </RadioGroup>
      )}
    />
  );
}

/**
 * The duplicate-risk confirmation, shown exactly when a selected delivery was acknowledged,
 * timed out or has an unknown outcome.
 */
function RiskConfirmation({ subject }: { subject: ResendSubject }) {
  return (
    <Controller<ResendValues, "confirm">
      name="confirm"
      rules={{
        validate: (value, values) =>
          !needsRiskConfirmation(subject, values.destinations) ||
          value ||
          "Confirm the duplicate risk to resend these deliveries.",
      }}
      render={({ field, fieldState }) => (
        <div>
          <Alert tone="warning" title="The destination may already have this message" compact>
            <p>
              A selected delivery was acknowledged, got no acknowledgement in time, or ended with an
              unknown outcome. The destination may have processed the message already, and would
              receive it a second time.
            </p>
          </Alert>
          <Checkbox isSelected={field.value} onChange={field.onChange}>
            I have checked the destination and accept that it may receive this message twice
          </Checkbox>
          {fieldState.error && <p className={styles.error}>{fieldState.error.message}</p>}
        </div>
      )}
    />
  );
}
