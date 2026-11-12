import { type ReactNode, useEffect, useRef } from "react";
import { useFormContext } from "react-hook-form";
import { Facts } from "@/components/Facts";
import { FieldGrid, FieldSection } from "@/components/Form";
import { TextInput } from "@/components/FormFields";
import { uniqueSlug } from "./configDocument";
import { FormCheckbox } from "./FormCheckbox";
import { ID_FORMAT } from "./validators";

/**
 * While the id has not been typed, suggests one from the label. Synchronizes with the form
 * library's change events, which live outside React.
 */
function useIdSuggestion(takenIds: ReadonlySet<string>, isActive: boolean) {
  // The form's methods are stable; the context value that holds them is not.
  const { watch, setValue, getValues } = useFormContext();
  const typed = useRef(false);
  useEffect(() => {
    if (!isActive) return;
    const subscription = watch((_values, { name, type }) => {
      if (name === "id" && type === "change") typed.current = true;
      if (name === "label" && !typed.current) {
        setValue("id", uniqueSlug(String(getValues("label") ?? ""), takenIds));
      }
    });
    return () => subscription.unsubscribe();
  }, [watch, setValue, getValues, takenIds, isActive]);
}

interface GeneralFieldsProps {
  isNew: boolean;
  /** Ids already in the configuration, so a suggested id is unique. */
  takenIds: ReadonlySet<string>;
  /** Creating: the choice of kind, first in the section. */
  kindChoice?: ReactNode;
  /** Editing: what cannot be changed any more (kind, id). */
  fixed?: readonly { label: string; value: ReactNode }[];
}

/** The General section: label, id (create only), enabled. */
export function GeneralFields({ isNew, takenIds, kindChoice, fixed }: GeneralFieldsProps) {
  useIdSuggestion(takenIds, isNew);
  return (
    <FieldSection title="General">
      {isNew ? kindChoice : fixed && <Facts layout="compact" items={fixed} />}
      <FieldGrid>
        <TextInput
          name="label"
          label="Label"
          isRequired
          description="The name shown to operators."
        />
        {isNew && (
          <TextInput
            name="id"
            label="Id"
            isRequired
            format={ID_FORMAT}
            description="Suggested from the label. Fixed once saved."
          />
        )}
      </FieldGrid>
      <FormCheckbox name="enabled" label="Enabled" />
    </FieldSection>
  );
}
