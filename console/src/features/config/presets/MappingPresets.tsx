// Mapping presets for one destination: a ready mapping for the pair of protocols, with its
// limits and an example. Rules are added after the operator's own, or replace them only when the
// operator chooses that and sees what goes; a table they already have is never overwritten.
import { useState } from "react";
import { useFormContext, useWatch } from "react-hook-form";
import type { Protocol } from "@/api/types";
import { Alert } from "@/components/Alert";
import { Button } from "@/components/Button";
import { Dialog } from "@/components/Dialog";
import { Select } from "@/components/Select";
import { useToast } from "@/components/Toast";
import type { RequireFormValues } from "../requireForm";
import type { CodeTableFormValues, RuleFormValues } from "../ruleForm";
import { emptySettings, type SettingsFormValues } from "../settingsForm";
import {
  type MappingMode,
  type MappingPlan,
  planMapping,
  type SettingChange,
  type SourceHint,
  sourceHints,
} from "./apply";
import { SettingsPlanTable, show } from "./ConnectorPresets";
import { connectorProfiles, type MappingPreset, presetsFor } from "./catalog";
import styles from "./Presets.module.css";

const MODES = [
  { id: "add", label: "Keep my rules and add the preset's after them" },
  { id: "replace", label: "Replace my rules and tables with the preset's" },
];

const count = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

interface MappingPresetsProps {
  /** The form path of the destination: `destinations.0`. */
  root: string;
  /** Names the destination in labels: `Destination 1`. */
  owner: string;
}

type Getter = ReturnType<typeof useFormContext>["getValues"];
type Setter = ReturnType<typeof useFormContext>["setValue"];

/** What the destination holds now, as a preset sees it. Nothing is read while the dialog is closed. */
function destinationValues(getValues: Getter, root: string, open: boolean) {
  const read = <T,>(name: string, closed: T): T =>
    open ? (getValues(`${root}.${name}`) as T) : closed;
  return {
    mappings: read<RuleFormValues[]>("mappings", []),
    codeTables: read<CodeTableFormValues[]>("codeTables", []),
    requireSingleGroup: read<boolean>("requireSingleGroup", false) === true,
    require: read<RequireFormValues[]>("require", []),
    settings: read<SettingsFormValues>("settings", emptySettings()),
  };
}

/** Writes the plan into the destination's own fields, marking each as changed. */
function writePlan(
  setValue: Setter,
  root: string,
  plan: MappingPlan,
  changing: readonly SettingChange[],
): void {
  const put = (name: string, value: unknown) =>
    setValue(`${root}.${name}`, value, { shouldDirty: true });
  put("mappings", plan.mappings);
  put("codeTables", plan.codeTables);
  put("requireSingleGroup", plan.requireSingleGroup);
  put("require", plan.require);
  for (const change of changing) put(`settings.${change.field}`, change.becomes);
}

export function MappingPresets({ root, owner }: MappingPresetsProps) {
  const { control, getValues, setValue } = useFormContext();
  const toast = useToast();
  const source = useWatch({ control, name: "source.protocol" }) as Protocol | null;
  const destination = useWatch({ control, name: `${root}.protocol` }) as Protocol | null;
  const presets = presetsFor(source, destination);
  const [open, setOpen] = useState(false);
  const [chosen, setChosen] = useState<string | null>(null);
  const [mode, setMode] = useState<MappingMode>("add");
  if (presets.length === 0) return null;
  const preset = presets.find((each) => each.id === chosen) ?? null;
  const current = destinationValues(getValues, root, open);
  const hasOwn = current.mappings.length > 0 || current.codeTables.length > 0;
  const effective: MappingMode = hasOwn ? mode : "add";
  const plan = preset === null || !open ? null : planMapping(preset, current, effective);
  const close = () => setOpen(false);
  const changing = plan?.settings.changes.filter((change) => change.changes) ?? [];
  return (
    <>
      <Button
        onPress={() => {
          setChosen(presets[0]?.id ?? null);
          setMode("add");
          setOpen(true);
        }}
      >
        {`Start ${owner.toLowerCase()} from a preset`}
      </Button>
      {open && (
        <Dialog
          title={`Mapping presets for ${owner.toLowerCase()}`}
          description="A preset is a starting point: it adds ordinary rules and tables that you can read and change afterwards."
          isOpen
          onOpenChange={(isOpen) => {
            if (!isOpen) close();
          }}
          size="wide"
          onSubmit={() => {
            if (plan === null || preset === null) return;
            writePlan(setValue, root, plan, changing);
            toast.show({
              tone: "success",
              title: `${preset.label} applied`,
              description: `${count(plan.rulesAdded, "rule")} and ${count(plan.tablesAdded.length, "table")} added to ${owner.toLowerCase()}${plan.rulesRemoved + plan.tablesRemoved > 0 ? `; ${count(plan.rulesRemoved, "rule")} and ${count(plan.tablesRemoved, "table")} replaced` : ""}. Add your test codes to the code table.`,
            });
            close();
          }}
          actions={
            <>
              <Button onPress={close}>Cancel</Button>
              <Button
                type="submit"
                variant={effective === "replace" ? "danger" : "primary"}
                isDisabled={plan === null}
              >
                {effective === "replace" ? "Replace and apply" : "Add to the mapping"}
              </Button>
            </>
          }
        >
          <Select
            label="Preset"
            name="preset"
            options={presets.map((each) => ({ id: each.id, label: each.label }))}
            value={chosen}
            onChange={setChosen}
            isRequired
          />
          {preset && plan === null && (
            <Alert tone="danger" title="This preset cannot be shown here">
              It uses something this version of the console does not know. It is not applied;
              nothing was changed.
            </Alert>
          )}
          {preset && plan && (
            <PresetDetails
              preset={preset}
              plan={plan}
              owner={owner}
              mode={mode}
              onModeChange={setMode}
              hasOwn={hasOwn}
              effective={effective}
              hints={sourceHints(
                preset,
                getValues("source.settings") as SettingsFormValues,
                connectorProfiles,
              )}
            />
          )}
        </Dialog>
      )}
    </>
  );
}

interface PresetDetailsProps {
  preset: MappingPreset;
  plan: MappingPlan;
  owner: string;
  mode: MappingMode;
  onModeChange: (mode: MappingMode) => void;
  /** The destination already has rules or tables of its own. */
  hasOwn: boolean;
  effective: MappingMode;
  /** Source settings the example was read with that the source does not have. */
  hints: readonly SourceHint[];
}

/** What a preset is, and what applying it to this destination would do. */
function PresetDetails({
  preset,
  plan,
  owner,
  mode,
  onModeChange,
  hasOwn,
  effective,
  hints,
}: PresetDetailsProps) {
  return (
    <>
      <p>{preset.summary}</p>
      {preset.notes.length > 0 && (
        <ul className={styles.notes}>
          {preset.notes.map((note) => (
            <li key={note}>{note}</li>
          ))}
        </ul>
      )}
      {hasOwn && (
        <Select
          label="You already have rules or tables"
          name="mode"
          options={MODES}
          value={mode}
          onChange={(next) => onModeChange(next === "replace" ? "replace" : "add")}
          isRequired
        />
      )}
      <p role="status">
        {`This adds ${count(plan.rulesAdded, "rule")} and ${count(plan.tablesAdded.length, "table")}${plan.tablesAdded.length > 0 ? ` (${plan.tablesAdded.join(", ")})` : ""}.`}
        {plan.tablesKept.length > 0 &&
          ` Your table ${plan.tablesKept.join(", ")} is kept as it is; the preset's rules use it.`}
      </p>
      {hints.map(({ change, profile }) => (
        <Alert
          key={change.field}
          tone="info"
          title={`The example reads the source with ${change.label} ${show(change.becomes)}`}
        >
          {`The source has ${show(change.now)}. ${profile ? `Apply the source preset "${profile}" or set` : "Set"} ${change.label} in the source's own fields to match, or Messages cannot search these results by the sample id.`}
        </Alert>
      ))}
      {plan.requirementsAdded > 0 && (
        <p>
          {`It also adds ${count(plan.requirementsAdded, "source requirement")}: checks on the message received that fail the mapping, with the reason, when the message does not fit what the rules assume.`}
        </p>
      )}
      {plan.requireSingleGroup && (
        <p>
          It also ticks Refuse messages with several patients or orders: its rules assume one sample
          per message.
        </p>
      )}
      {effective === "replace" && (
        <Alert tone="warning" title="This removes what you have here">
          {`${count(plan.rulesRemoved, "rule")} and ${count(plan.tablesRemoved, "table")} of ${owner.toLowerCase()} are replaced. Nothing is saved until you save and apply the channel; leaving the editor without saving drops the change.`}
        </Alert>
      )}
      {plan.settings.changes.length > 0 && (
        <>
          <p>It also sets, on this destination:</p>
          <SettingsPlanTable changes={plan.settings.changes} unknown={plan.settings.unknown} />
        </>
      )}
      <details>
        <summary>Example input and output</summary>
        <p>Input</p>
        <pre className={styles.example}>{preset.sample.input.replaceAll("\r", "\n")}</pre>
        <p>Output with the preset's settings</p>
        <pre className={styles.example}>{preset.sample.output.replaceAll("\r", "\n")}</pre>
      </details>
      <p className={styles.evidence}>Based on: {preset.evidence}</p>
    </>
  );
}
