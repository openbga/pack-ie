import { Check, ChevronDown } from "lucide-react";
import { type ComponentProps, type Ref, useContext } from "react";
import {
  Button,
  ComboBox,
  ComboBoxStateContext,
  Input,
  ListBox,
  ListBoxItem,
  Popover,
  Text,
} from "react-aria-components";
import { FieldHelp } from "@/components/Form/FieldHelp";
import { FieldLabel } from "@/components/Form/FieldLabel";
import formStyles from "@/components/Form/Form.module.css";
import selectStyles from "@/components/Select/Select.module.css";
import styles from "./RecordPicker.module.css";

/** One existing record to choose. */
export interface RecordOption {
  id: string;
  label: string;
  /** What tells near-duplicates apart, such as its kind and state; shown and searched. */
  description?: string;
}

interface RecordPickerProps {
  label: string;
  /** Also the key for engine field errors shown by `Form`. */
  name: string;
  /** The records to choose from; typing narrows them by label and description. */
  options: readonly RecordOption[];
  value: string | null;
  onChange: (value: string | null) => void;
  isRequired?: boolean;
  isDisabled?: boolean;
  description?: string;
  errorMessage?: string | undefined;
  /** The text input, so a form can focus the field when it is invalid. */
  inputRef?: Ref<HTMLInputElement>;
}

/** The option as the field shows it once chosen, and as typing searches it. */
const optionText = ({ label, description }: RecordOption) =>
  description ? `${label} (${description})` : label;

/**
 * Chooses one existing record by typing part of its label or description, for lists too
 * long to scan (more than 15 options). Free text is never kept: leaving the field restores
 * the chosen record's text. The chosen record stays selected and marked in the list.
 */
export function RecordPicker({
  label,
  name,
  options,
  value,
  onChange,
  description,
  errorMessage,
  isRequired,
  isDisabled,
  inputRef,
}: RecordPickerProps) {
  return (
    <ComboBox<RecordOption>
      name={name}
      isDisabled={isDisabled}
      selectedKey={value}
      onSelectionChange={(key) => onChange(key === null ? null : String(key))}
      defaultItems={options}
      validationBehavior="aria"
      allowsEmptyCollection
      isRequired={isRequired}
      isInvalid={errorMessage === undefined ? undefined : true}
      className={formStyles.field}
    >
      <FieldLabel isRequired={isRequired}>{label}</FieldLabel>
      <div className={styles.control}>
        <OpeningInput
          ref={inputRef}
          autoCorrect="off"
          autoCapitalize="off"
          spellCheck={false}
          placeholder="Type to search…"
          className={`${formStyles.control} ${styles.input}`}
        />
        <Button className={styles.opener}>
          <ChevronDown aria-hidden="true" className={selectStyles.chevron} />
        </Button>
      </div>
      <FieldHelp description={description} errorMessage={errorMessage} />
      <Popover className={`${selectStyles.popover} ${styles.popover}`}>
        <ListBox
          className={selectStyles.listBox}
          renderEmptyState={() => <p className={styles.empty}>No matches</p>}
        >
          {(option: RecordOption) => (
            <ListBoxItem
              id={option.id}
              textValue={optionText(option)}
              className={`${selectStyles.option} ${styles.option}`}
            >
              <Check aria-hidden="true" className={styles.selectedIcon} />
              <Text slot="label">{option.label}</Text>
              {option.description && (
                <Text slot="description" className={styles.description}>
                  {option.description}
                </Text>
              )}
            </ListBoxItem>
          )}
        </ListBox>
      </Popover>
    </ComboBox>
  );
}

/**
 * The text input; a click anywhere in it opens the full list, as the arrow and a `Select`
 * do, and typing still narrows it. Keyboard focus alone does not open it. The chosen text is
 * selected on entering the field, so typing replaces it instead of extending it.
 */
function OpeningInput(props: ComponentProps<typeof Input>) {
  const state = useContext(ComboBoxStateContext);
  return (
    <Input
      {...props}
      onFocus={(event) => event.currentTarget.select()}
      onClick={(event) => {
        event.currentTarget.select();
        if (state && !state.isOpen) state.open(null, "manual");
      }}
    />
  );
}
