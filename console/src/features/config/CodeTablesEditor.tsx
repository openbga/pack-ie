import { useFieldArray, useFormContext, useWatch } from "react-hook-form";
import { FieldList } from "@/components/FieldList";
import { TextInput } from "@/components/FormFields";
import type { CodeTableFormValues } from "./ruleForm";
import { TableTextDialog } from "./TableTextDialog";

interface TablesProps {
  /** The form path of the destination holding the tables: `destinations.0`. */
  root: string;
  /** Names the destination in labels: `Destination 1`. */
  owner: string;
}

function TableRows({ root, owner, table }: TablesProps & { table: number }) {
  const { control } = useFormContext();
  const name = `${root}.codeTables.${table}.rows`;
  const { fields, append, remove, replace } = useFieldArray({ control, name });
  const tableLabel = `${owner.toLowerCase()} table ${table + 1}`;
  return (
    <FieldList
      name={name}
      legend={`Rows of ${tableLabel}`}
      emptyText="No rows yet."
      items={fields}
      itemLabel={(row) => `Row ${row + 1} of ${tableLabel}`}
      addLabel={`Add row to ${tableLabel}`}
      onAdd={() => append({ code: "", text: "" })}
      onRemove={remove}
      extraActions={<TableTextDialog name={name} tableLabel={tableLabel} onApply={replace} />}
      renderItem={(row) => (
        <>
          <TextInput
            name={`${name}.${row}.code`}
            label="Code"
            isRequired
            description="The text as it arrives."
          />
          <TextInput name={`${name}.${row}.text`} label="Text" description="What it becomes." />
        </>
      )}
    />
  );
}

function TableName({ root, table }: { root: string; table: number }) {
  const { control } = useFormContext();
  const tables = useWatch({ control, name: `${root}.codeTables` }) as CodeTableFormValues[];
  const taken = (name: string) => tables.filter((other) => other.name.trim() === name).length > 1;
  return (
    <TextInput
      name={`${root}.codeTables.${table}.name`}
      label="Table name"
      isRequired
      description="What a lookup rule chooses."
      validate={(name) => !taken(name) || "Another table already has this name."}
    />
  );
}

/** The named tables of code → text that the `lookup` rules of one destination read. */
export function CodeTablesEditor({ root, owner }: TablesProps) {
  const { control } = useFormContext();
  const name = `${root}.codeTables`;
  const { fields, append, remove } = useFieldArray({ control, name });
  return (
    <FieldList
      name={name}
      legend={`${owner} code tables`}
      emptyText="No code tables. Add one to use it in a lookup rule."
      items={fields}
      itemLabel={(index) => `${owner} table ${index + 1}`}
      addLabel={`Add ${owner.toLowerCase()} code table`}
      onAdd={() => append({ name: "", rows: [] })}
      onRemove={remove}
      renderItem={(index) => (
        <>
          <TableName root={root} table={index} />
          <TableRows root={root} owner={owner} table={index} />
        </>
      )}
    />
  );
}
