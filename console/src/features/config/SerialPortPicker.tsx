import { Check, ChevronDown } from "lucide-react";
import { type ComponentProps, type Ref, useContext, useState } from "react";
import {
  Button,
  ComboBox,
  ComboBoxStateContext,
  Input,
  ListBox,
  ListBoxItem,
  Popover,
} from "react-aria-components";
import type { SerialPortInfo, SerialPortKind, SerialPortUse } from "@/api/types";
import { FieldHelp } from "@/components/Form/FieldHelp";
import { FieldLabel } from "@/components/Form/FieldLabel";
import formStyles from "@/components/Form/Form.module.css";
import recordStyles from "@/components/RecordPicker/RecordPicker.module.css";
import selectStyles from "@/components/Select/Select.module.css";
import styles from "./SerialPortPicker.module.css";

const KIND_LABELS: Record<SerialPortKind, string> = {
  usb: "USB",
  pci: "PCI",
  bluetooth: "Bluetooth",
  unknown: "Other",
};

const hex = (id: number) => id.toString(16).padStart(4, "0");

/** What identifies a port besides its name: bus, device text, ids; missing parts are left out. */
export function portIdentity(port: SerialPortInfo): string {
  const device = [port.product, port.manufacturer && `(${port.manufacturer})`]
    .filter(Boolean)
    .join(" ");
  const ids =
    port.vid !== null && port.pid !== null
      ? `${hex(port.vid)}:${hex(port.pid)}`
      : port.vid !== null
        ? `vendor ${hex(port.vid)}`
        : port.pid !== null
          ? `product ${hex(port.pid)}`
          : "";
  return [KIND_LABELS[port.kind], device, ids, port.serialNumber && `serial ${port.serialNumber}`]
    .filter(Boolean)
    .join(" · ");
}

/** The endpoints naming the port other than the one being edited (all of them when creating). */
export function otherUses(port: SerialPortInfo, ownEndpointId: string | undefined) {
  return port.usedBy.filter((use) => use.endpointId !== ownEndpointId);
}

/** `used by <endpoint>`, and `open by <channel>` where a running channel holds the port. */
function markerOf(uses: readonly SerialPortUse[]): string {
  const holders = uses.flatMap((use) => (use.heldBy === null ? [] : [use.heldBy]));
  const by = `used by ${uses.map((use) => use.endpointLabel).join(", ")}`;
  return holders.length === 0 ? by : `${by} · open by ${holders.join(", ")}`;
}

/** One entry of the list: a port found, or the name typed. */
interface PortOption {
  id: string;
  name: string;
  /** What the search looks in, besides the name. */
  search: string;
  identity: string;
  marker: string;
}

/** The key of the entry that keeps the typed name. */
const TYPED = "\u0000typed";

interface SerialPortPickerProps {
  label: string;
  /** Also the key for engine field errors shown by `Form`. */
  name: string;
  /** The ports found on the computer; empty while loading or when none are found. */
  ports: readonly SerialPortInfo[];
  /** The edited endpoint, whose own use of a port is not a marker. */
  ownEndpointId: string | undefined;
  /** The port name as typed or chosen. */
  value: string;
  onChange: (value: string) => void;
  isRequired?: boolean;
  isDisabled?: boolean;
  placeholder: string;
  description?: string;
  errorMessage?: string | undefined;
  /** The text input, so a form can focus the field when it is invalid. */
  inputRef?: Ref<HTMLInputElement>;
}

/**
 * Chooses a serial port found on this computer, or keeps any name typed: a port that is not
 * plugged in now, a virtual link, a path. Typing narrows the list by name and device text; the
 * typed name is always offered as the last entry, and is what the field holds either way.
 */
export function SerialPortPicker({
  label,
  name,
  ports,
  ownEndpointId,
  value,
  onChange,
  isRequired,
  isDisabled,
  placeholder,
  description,
  errorMessage,
  inputRef,
}: SerialPortPickerProps) {
  // The list shows every port until something is typed; opening it again shows all once more.
  const [isNarrowed, setNarrowed] = useState(false);
  const found = ports.some((port) => port.name === value);
  const needle = value.trim().toLowerCase();
  const options: PortOption[] = ports.flatMap((port) => {
    const identity = portIdentity(port);
    const search = `${port.name} ${identity}`.toLowerCase();
    if (isNarrowed && !search.includes(needle)) return [];
    const uses = otherUses(port, ownEndpointId);
    return [
      {
        id: port.name,
        name: port.name,
        search,
        identity,
        marker: uses.length === 0 ? "" : markerOf(uses),
      },
    ];
  });
  if (needle !== "" && !found) {
    options.push({
      id: TYPED,
      name: `Use “${value.trim()}” anyway`,
      search: value,
      identity: "Not found on this computer",
      marker: "",
    });
  }
  return (
    <ComboBox<PortOption>
      name={name}
      isDisabled={isDisabled}
      items={options}
      inputValue={value}
      onInputChange={(text) => {
        if (text === value) return;
        setNarrowed(true);
        onChange(text);
      }}
      selectedKey={found ? value : needle === "" ? null : TYPED}
      onSelectionChange={(key) => {
        if (key !== null && key !== TYPED) onChange(String(key));
      }}
      onOpenChange={(isOpen, trigger) => {
        if (isOpen && trigger === "manual") setNarrowed(false);
      }}
      allowsCustomValue
      allowsEmptyCollection
      formValue="text"
      validationBehavior="aria"
      isRequired={isRequired}
      isInvalid={errorMessage === undefined ? undefined : true}
      className={formStyles.field}
    >
      <FieldLabel isRequired={isRequired}>{label}</FieldLabel>
      <div className={recordStyles.control}>
        <OpeningInput
          ref={inputRef}
          autoCorrect="off"
          autoCapitalize="off"
          spellCheck={false}
          placeholder={placeholder}
          className={`${formStyles.control} ${recordStyles.input}`}
        />
        <Button className={recordStyles.opener}>
          <ChevronDown aria-hidden="true" className={selectStyles.chevron} />
        </Button>
      </div>
      <FieldHelp description={description} errorMessage={errorMessage} />
      <Popover className={`${selectStyles.popover} ${recordStyles.popover}`}>
        <ListBox
          className={selectStyles.listBox}
          renderEmptyState={() => (
            <p className={recordStyles.empty}>
              No serial ports found on this computer. Type a name to use it.
            </p>
          )}
        >
          {(option: PortOption) => (
            <ListBoxItem
              id={option.id}
              textValue={option.id === TYPED ? value : option.name}
              className={`${selectStyles.option} ${recordStyles.option} ${styles.option}`}
            >
              <Check aria-hidden="true" className={recordStyles.selectedIcon} />
              <span className={styles.text}>
                <span>{option.name}</span>
                <span className={styles.details}>
                  {option.identity && <span>{option.identity}</span>}
                  {option.marker && <strong className={styles.marker}>{option.marker}</strong>}
                </span>
              </span>
            </ListBoxItem>
          )}
        </ListBox>
      </Popover>
    </ComboBox>
  );
}

/**
 * The text input; a click anywhere in it opens the full list, as the arrow does. The list opens
 * once the focus that came with the click has settled: opened at once, a click on a field that
 * did not have the focus closes it again.
 */
function OpeningInput(props: ComponentProps<typeof Input>) {
  const state = useContext(ComboBoxStateContext);
  return (
    <Input
      {...props}
      onClick={() => {
        if (state && !state.isOpen) setTimeout(() => state.open(null, "manual"), 0);
      }}
    />
  );
}
