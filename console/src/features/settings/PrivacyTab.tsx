import { useState } from "react";
import { Alert } from "@/components/Alert";
import { Button } from "@/components/Button";
import { FieldGrid } from "@/components/Form";
import { Section } from "@/components/Section";
import { TextField } from "@/components/TextField";
import { useToast } from "@/components/Toast";
import { DEFAULT_IDLE_MINUTES, MAX_IDLE_MINUTES, MIN_IDLE_MINUTES, useIdle } from "@/lib/idle";
import styles from "@/styles/layout.module.css";

/**
 * Idle protection: after this many minutes without activity the console clears the message
 * content it holds, leaves the message detail, closes audit event details and asks to
 * Continue. The setting belongs to this console on this computer, not to the engine; there is
 * no login.
 */
export function PrivacyTab() {
  const { minutes, setMinutes } = useIdle();
  const toast = useToast();
  const [text, setText] = useState(String(minutes));
  const parsed = Number(text);
  const valid =
    text.trim() !== "" &&
    Number.isInteger(parsed) &&
    parsed >= MIN_IDLE_MINUTES &&
    parsed <= MAX_IDLE_MINUTES;
  const dirty = valid && parsed !== minutes;
  function save() {
    setMinutes(parsed);
    toast.show({
      tone: "success",
      title: "Idle protection saved",
      description: `Content is hidden after ${parsed} ${parsed === 1 ? "minute" : "minutes"} without activity.`,
    });
  }
  return (
    <div className={styles.stack}>
      <Alert tone="info" compact>
        This setting is kept by this console on this computer. The engine does not know it, and
        there is no login: the console hides content, it does not lock the computer.
      </Alert>
      <Section title="Idle protection">
        <p className={styles.hint}>
          Message bodies, parsed message fields and audit event details can hold patient
          information. After the idle time the console clears the message content it has loaded,
          leaves the message detail for the message list, closes the audit event detail and covers
          the screen until someone chooses Continue. Opening a message body again reads it from the
          engine again, and the engine records that view.
        </p>
        <FieldGrid>
          <TextField
            label="Hide content after (minutes)"
            name="idleMinutes"
            value={text}
            onChange={setText}
            inputMode="numeric"
            description={`${MIN_IDLE_MINUTES} to ${MAX_IDLE_MINUTES} minutes. Default ${DEFAULT_IDLE_MINUTES}.`}
            errorMessage={
              valid
                ? undefined
                : `Enter a whole number from ${MIN_IDLE_MINUTES} to ${MAX_IDLE_MINUTES}.`
            }
          />
        </FieldGrid>
        <div>
          <Button variant="primary" isDisabled={!dirty} onPress={save}>
            Save
          </Button>
        </div>
      </Section>
    </div>
  );
}
