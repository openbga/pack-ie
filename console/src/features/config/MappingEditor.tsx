import { useFormContext, useWatch } from "react-hook-form";
import { Alert } from "@/components/Alert";
import { CodeTablesEditor } from "./CodeTablesEditor";
import styles from "./config.module.css";
import { FormCheckbox } from "./FormCheckbox";
import { MappingRules, topScope } from "./MappingRules";
import { MappingPresets } from "./presets/MappingPresets";
import { RequireEditor } from "./RequireEditor";

interface MappingEditorProps {
  /** The form path of the destination: `destinations.0`. */
  root: string;
  /** Names the destination in labels: `Destination 1`. */
  owner: string;
}

/** Ordered mapping rules of one destination, applied top to bottom, and the tables they read. */
export function MappingEditor({ root, owner }: MappingEditorProps) {
  const { control } = useFormContext();
  const hidden = useWatch({ control, name: `${root}.mappingHidden` }) === true;
  if (hidden) {
    return (
      <div className={styles.nested}>
        <Alert tone="warning" title={`${owner} has a mapping this console cannot show`}>
          Its mapping uses an operation or a field this version of the console does not know. It
          keeps working and is saved unchanged, but its rules are not listed here. To change it,
          edit runtime.json and apply it with the command line.
        </Alert>
      </div>
    );
  }
  return (
    <>
      <div className={styles.nested}>
        <MappingPresets root={root} owner={owner} />
      </div>
      <div className={styles.nested}>
        <FormCheckbox
          name={`${root}.requireSingleGroup`}
          label="Refuse messages with several patients or orders"
          description="A message with more than one patient, order or specimen fails this mapping with a transform error, so a calculated result is never placed under the wrong one. Tick it when the rules assume one sample per message."
        />
      </div>
      <div className={styles.nested}>
        <RequireEditor root={root} owner={owner} />
      </div>
      <div className={styles.nested}>
        <MappingRules name={`${root}.mappings`} scope={topScope(root, owner)} />
      </div>
      <div className={styles.nested}>
        <CodeTablesEditor root={root} owner={owner} />
      </div>
    </>
  );
}
