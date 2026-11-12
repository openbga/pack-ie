// The AU connector's settings (docs/spec/configuration.md §7.2), copied by the laboratory from
// the instrument's online parameter sheet. Every field is required and nothing is preset.
import type { ReactNode } from "react";
import { useFormContext } from "react-hook-form";
import { FieldGrid } from "@/components/Form";
import { ChoiceInput, TextInput } from "@/components/FormFields";
import { Inset } from "@/components/Inset";
import {
  controlBytes,
  etbAllowed,
  RACK_DIGITS,
  RESULT_DIGITS,
  TEST_CODE_DIGITS,
  timeoutMs,
  WIRE_LENGTHS,
} from "./auForm";
import styles from "./config.module.css";
import { FormCheckbox } from "./FormCheckbox";
import { wholeNumber } from "./validators";

const PROFILES = [
  { id: "au400", label: "AU400" },
  { id: "au640", label: "AU640" },
];
const CLASSES = [
  { id: "a", label: "Class a" },
  { id: "b", label: "Class b" },
];
const asOptions = (ids: readonly string[]) => ids.map((id) => ({ id, label: id }));
const PATIENT_INFO_FIELDS = [0, 1, 2, 3, 4, 5];
const sampleDigits = wholeNumber({ min: 1, max: 20 });
const patientDigits = wholeNumber({ min: 0, max: 20 });

function Group({ title, children }: { title: string; children: ReactNode }) {
  return (
    <fieldset className={styles.group}>
      <legend className={styles.legend}>{title}</legend>
      <FieldGrid>{children}</FieldGrid>
    </fieldset>
  );
}

function FramingGroup({ prefix }: { prefix: string }) {
  const { getValues } = useFormContext();
  return (
    <Group title="Framing">
      <TextInput
        name={`${prefix}.framingStart`}
        label="Start bytes"
        isRequired
        validate={controlBytes}
        description="Control bytes that open a frame: one or two numbers from 1 to 31, such as 2."
      />
      <TextInput
        name={`${prefix}.framingEnd`}
        label="End bytes"
        isRequired
        validate={controlBytes}
        description="Control bytes that close a frame, such as 3. Never 6 (ACK) or 21 (NAK)."
      />
      <FormCheckbox
        name={`${prefix}.bcc`}
        label="Check byte (BCC)"
        description="One XOR check byte follows each frame."
      />
      <FormCheckbox
        name={`${prefix}.useEtb`}
        label="ETB between blocks"
        description="Marks intermediate blocks with ETB; only when the end bytes are 3."
        validate={(checked) => etbAllowed(checked, String(getValues(`${prefix}.framingEnd`)))}
      />
      <ChoiceInput
        name={`${prefix}.maxWireLen`}
        label="Maximum wire length"
        options={asOptions(WIRE_LENGTHS)}
        isRequired
        description="Bytes per block on the wire."
      />
      <ChoiceInput
        name={`${prefix}.framingClass`}
        label="Class"
        options={CLASSES}
        isRequired
        description="Class a: no ACK or NAK when receiving. Class b: ACK after the message is stored."
      />
    </Group>
  );
}

function FormatGroup({ prefix }: { prefix: string }) {
  return (
    <Group title="Message format">
      <FormCheckbox
        name={`${prefix}.unitNo`}
        label="Unit number"
        description="Incoming messages carry a two-digit unit number."
      />
      <ChoiceInput
        name={`${prefix}.rackDigits`}
        label="Rack digits"
        options={asOptions(RACK_DIGITS)}
        isRequired
        description="Digits of the rack number; 0 when there is none."
      />
      <TextInput
        name={`${prefix}.sampleIdDigits`}
        label="Sample ID digits"
        isRequired
        validate={sampleDigits}
        description="1 to 20."
      />
      <FormCheckbox name={`${prefix}.sex`} label="Sex" description="The patient header has sex." />
      <FormCheckbox name={`${prefix}.age`} label="Age" description="The patient header has age." />
      <FormCheckbox
        name={`${prefix}.month`}
        label="Month"
        description="The patient header has the month."
      />
      {PATIENT_INFO_FIELDS.map((index) => (
        <TextInput
          key={index}
          name={`${prefix}.patientInfoDigits.${index}`}
          label={`Patient information ${index + 1} digits`}
          isRequired
          validate={patientDigits}
          description={index === 0 ? "0 to 20 for each of the six fields." : undefined}
        />
      ))}
      <ChoiceInput
        name={`${prefix}.resultDigits`}
        label="Result digits"
        options={asOptions(RESULT_DIGITS)}
        isRequired
        description="Digits of each result value."
      />
      <ChoiceInput
        name={`${prefix}.testCodeDigits`}
        label="Test code digits"
        options={asOptions(TEST_CODE_DIGITS)}
        isRequired
        description="Width of a test code in result items and requests, as the instrument's parameter sheet says."
      />
    </Group>
  );
}

function TimingGroup({ prefix }: { prefix: string }) {
  return (
    <Group title="Timing">
      <TextInput
        name={`${prefix}.frameTimeoutMs`}
        label="Frame timeout (ms)"
        isRequired
        validate={timeoutMs}
        description="Time allowed for one frame from its first byte. 100 to 9900, in steps of 100."
      />
      <TextInput
        name={`${prefix}.blockTimeoutMs`}
        label="Block timeout (ms)"
        validate={timeoutMs}
        description="Wait for the next block of an unfinished sample. 100 to 9900, in steps of 100. Leave blank for the default, 9900."
      />
    </Group>
  );
}

/** The AU settings of one connector; `prefix` is the form path of `settings.au`. */
export function AuFields({ prefix }: { prefix: string }) {
  return (
    <div className={styles.nested}>
      <Inset as="fieldset">
        <legend className={styles.legend}>AU instrument settings</legend>
        <p className={styles.note}>
          Copy these from the instrument's online parameter sheet; there are no defaults.
        </p>
        <Group title="Instrument">
          <ChoiceInput
            name={`${prefix}.profile`}
            label="Profile"
            options={PROFILES}
            isRequired
            description="The instrument model."
          />
        </Group>
        <FramingGroup prefix={prefix} />
        <FormatGroup prefix={prefix} />
        <TimingGroup prefix={prefix} />
      </Inset>
    </div>
  );
}
