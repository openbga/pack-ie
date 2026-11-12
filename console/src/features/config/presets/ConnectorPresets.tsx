// Instrument presets for one connector: named bundles of protocol settings. Applying one writes
// explicit values into the connector's own fields; the dialog lists each setting, what it is now
// and what it becomes, and only those that change are written.
import { useState } from "react";
import { useFormContext } from "react-hook-form";
import type { Protocol } from "@/api/types";
import { Button } from "@/components/Button";
import { Dialog } from "@/components/Dialog";
import { Select } from "@/components/Select";
import { useToast } from "@/components/Toast";
import type { SettingsFormValues } from "../settingsForm";
import { planProfile, type SettingChange } from "./apply";
import { type ConnectorRole, profilesFor } from "./catalog";
import styles from "./Presets.module.css";

export const show = (value: string | boolean): string =>
  typeof value === "boolean" ? (value ? "yes" : "no") : value === "" ? "(empty)" : value;

/** The settings a preset writes, one per line: now and becomes, with the unchanged ones named. */
export function SettingsPlanTable({
  changes,
  unknown,
}: {
  changes: readonly SettingChange[];
  unknown: readonly string[];
}) {
  return (
    <>
      <table className={styles.plan}>
        <caption className="visually-hidden">Settings the preset writes</caption>
        <thead>
          <tr>
            <th scope="col">Setting</th>
            <th scope="col">Now</th>
            <th scope="col">Becomes</th>
          </tr>
        </thead>
        <tbody>
          {changes.map((change) => (
            <tr key={change.field}>
              <th scope="row">{change.label}</th>
              <td>{show(change.now)}</td>
              <td>{change.changes ? <strong>{show(change.becomes)}</strong> : "already set"}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {unknown.length > 0 && (
        <p role="alert" className={styles.problem}>
          This version of the console does not know the setting {unknown.join(", ")}; it is not
          applied.
        </p>
      )}
    </>
  );
}

interface ConnectorPresetsProps {
  /** The form path of the connector: `source` or `destinations.0`. */
  prefix: string;
  protocol: Protocol | null;
  side: ConnectorRole;
}

/** A button and dialog offering the presets that suit this connector's protocol and role. */
export function ConnectorPresets({ prefix, protocol, side }: ConnectorPresetsProps) {
  const { getValues, setValue } = useFormContext();
  const toast = useToast();
  const profiles = profilesFor(protocol, side);
  const [open, setOpen] = useState(false);
  const [chosen, setChosen] = useState<string | null>(null);
  if (profiles.length === 0) return null;
  const profile = profiles.find((each) => each.id === chosen) ?? null;
  const current = getValues(`${prefix}.settings`) as SettingsFormValues;
  const plan = profile === null ? null : planProfile(profile, current);
  const changing = plan?.changes.filter((change) => change.changes) ?? [];
  const close = () => setOpen(false);
  return (
    <>
      <Button
        onPress={() => {
          setChosen(profiles[0]?.id ?? null);
          setOpen(true);
        }}
      >
        {`Instrument presets for ${prefix === "source" ? "the source" : "this destination"}`}
      </Button>
      {open && (
        <Dialog
          title="Instrument presets"
          description="A preset fills in settings that you can see and change afterwards. Nothing is applied by name, and only the settings listed change."
          isOpen
          onOpenChange={(isOpen) => {
            if (!isOpen) close();
          }}
          size="wide"
          onSubmit={() => {
            if (plan === null || profile === null) return;
            for (const change of changing) {
              setValue(`${prefix}.settings.${change.field}`, change.becomes, {
                shouldDirty: true,
              });
            }
            toast.show({
              tone: "success",
              title: `${profile.label} applied`,
              description: `${changing.length} ${changing.length === 1 ? "setting" : "settings"} changed. Review them in the connector's fields.`,
            });
            close();
          }}
          actions={
            <>
              <Button onPress={close}>Cancel</Button>
              <Button type="submit" variant="primary" isDisabled={changing.length === 0}>
                {changing.length === 0
                  ? "Nothing to change"
                  : `Apply ${changing.length} ${changing.length === 1 ? "setting" : "settings"}`}
              </Button>
            </>
          }
        >
          <Select
            label="Preset"
            name="preset"
            options={profiles.map((each) => ({ id: each.id, label: each.label }))}
            value={chosen}
            onChange={setChosen}
            isRequired
          />
          {profile && plan && (
            <>
              <p>{profile.summary}</p>
              <SettingsPlanTable changes={plan.changes} unknown={plan.unknown} />
              <p className={styles.evidence}>Based on: {profile.evidence}</p>
            </>
          )}
        </Dialog>
      )}
    </>
  );
}
