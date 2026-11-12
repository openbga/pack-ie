import { useState } from "react";
import { Link } from "react-aria-components";
import { Controller, useForm, useFormContext } from "react-hook-form";
import { useEngineApi } from "@/api/context";
import { useEngineMutation, useEngineQuery } from "@/api/hooks";
import type { SecretInfo } from "@/api/types";
import { Alert } from "@/components/Alert";
import { Button } from "@/components/Button";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { DataGrid, type DataGridColumn, type GridRowActions } from "@/components/DataGrid";
import { ErrorState } from "@/components/ErrorState";
import { FormDialog } from "@/components/FormDialog";
import { TextInput } from "@/components/FormFields";
import { QueryContent } from "@/components/QueryContent";
import { Section } from "@/components/Section";
import { StatusBadge } from "@/components/StatusBadge";
import { TextField } from "@/components/TextField";
import { useToast } from "@/components/Toast";
import { textFieldProps } from "@/lib/form";
import styles from "@/styles/layout.module.css";

const STORAGE_LABELS = {
  os_keystore: "OS key store",
  restricted_file: "Restricted file",
};

function storageLabel(storage: SecretInfo["storage"]): string {
  return storage === null ? "Not reported" : STORAGE_LABELS[storage];
}

function valueBadge(hasValue: SecretInfo["hasValue"]) {
  if (hasValue === null) return <StatusBadge tone="neutral">Unknown</StatusBadge>;
  return hasValue ? (
    <StatusBadge tone="neutral">Stored</StatusBadge>
  ) : (
    <StatusBadge tone="warning">Not set</StatusBadge>
  );
}

function valueOrder(hasValue: SecretInfo["hasValue"]): number {
  if (hasValue === null) return 1;
  return hasValue ? 2 : 0;
}

/** A password-type field for a secret's value: never shown again after it is saved. */
function SecretValueField() {
  const { control } = useFormContext();
  return (
    <Controller
      control={control}
      name="value"
      rules={{ validate: (value: string) => value !== "" || "Enter the value." }}
      render={(controller) => (
        <TextField
          {...textFieldProps(controller)}
          label="Value"
          type="password"
          isRequired
          autoComplete="new-password"
          description="Stored securely and never shown again. To change it later, set a new value."
        />
      )}
    />
  );
}

interface SecretValues {
  name: string;
  value: string;
}

/** Sets a value; with no `existing` name it also asks for the name of a new secret. */
function SetSecretDialog({ existing, onClose }: { existing: string | null; onClose: () => void }) {
  const toast = useToast();
  const form = useForm<SecretValues>({ defaultValues: { name: existing ?? "", value: "" } });
  const save = useEngineMutation(
    (api, values: SecretValues) => api.setSecret(values.name.trim(), values.value),
    { invalidates: ["listSecrets"] },
  );
  const title = existing === null ? "Add secret" : `Set value of ${existing}`;
  return (
    <FormDialog
      title={title}
      description="The value is stored in the OS key store where available, and is never shown again."
      submitLabel={existing === null ? "Add secret" : "Save value"}
      form={form}
      onSubmit={(event) =>
        void form.handleSubmit((values) =>
          save.mutate(values, {
            onSuccess: () => {
              toast.show({ tone: "success", title: `Secret ${values.name.trim()} saved` });
              onClose();
            },
          }),
        )(event)
      }
      command={save}
      errorTitle="The secret was not saved"
      onClose={onClose}
    >
      {existing === null && (
        <TextInput
          name="name"
          label="Name"
          isRequired
          format={{
            pattern: /^[A-Za-z0-9._-]+$/,
            message: "Use letters, digits, dots, hyphens and underscores.",
            description: "Endpoints refer to the secret by this name.",
          }}
        />
      )}
      <SecretValueField />
    </FormDialog>
  );
}

function DeleteSecretDialog({ secret, onClose }: { secret: SecretInfo; onClose: () => void }) {
  const toast = useToast();
  const remove = useEngineMutation((api, name: string) => api.deleteSecret(name), {
    invalidates: ["listSecrets"],
  });
  return (
    <ConfirmDialog
      title={`Delete secret ${secret.name}?`}
      isOpen
      onOpenChange={(isOpen) => {
        if (!isOpen) onClose();
      }}
      confirmLabel="Delete secret"
      isPending={remove.isPending}
      onConfirm={() =>
        remove.mutate(secret.name, {
          onSuccess: () => {
            toast.show({ tone: "success", title: `Secret ${secret.name} deleted` });
            onClose();
          },
        })
      }
    >
      <div className={styles.stack}>
        <p>The stored value is removed and cannot be recovered.</p>
        {secret.usedByEndpointIds.length > 0 && (
          <Alert tone="warning" title="Still in use">
            Endpoints that use this secret cannot sign in until a new value is set:{" "}
            {secret.usedByEndpointIds.join(", ")}.
          </Alert>
        )}
        {remove.error && <ErrorState title="The secret was not deleted" error={remove.error} />}
      </div>
    </ConfirmDialog>
  );
}

const COLUMNS: DataGridColumn<SecretInfo>[] = [
  {
    id: "name",
    header: "Name",
    identifier: true,
    cell: (row) => row.name,
    sortValue: (row) => row.name,
    defaultSort: "asc",
  },
  { id: "storage", header: "Storage", width: 180, cell: (row) => storageLabel(row.storage) },
  {
    id: "hasValue",
    header: "Value",
    width: 160,
    cell: (row) => valueBadge(row.hasValue),
    sortValue: (row) => valueOrder(row.hasValue),
  },
  {
    id: "usedBy",
    header: "Used by endpoints",
    wrap: true,
    cell: (row) =>
      row.usedByEndpointIds.length === 0 ? (
        "Not used"
      ) : (
        <span>
          {row.usedByEndpointIds.map((id, index) => (
            <span key={id}>
              {index > 0 && ", "}
              <Link href={`/endpoints/${id}`}>{id}</Link>
            </span>
          ))}
        </span>
      ),
  },
];

type Dialog = { kind: "set"; name: string | null } | { kind: "delete"; secret: SecretInfo };

const READ_ONLY_NOTE =
  "This engine build cannot store secrets through the console yet; set them with the OS key store or the secrets file.";

/** Names and states of stored secrets; values are never shown, only set or deleted. */
export function SecretsTab() {
  const { secretsWritable } = useEngineApi().capabilities;
  const secrets = useEngineQuery(["listSecrets"], (api) => api.listSecrets());
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const close = () => setDialog(null);
  const rowActions: GridRowActions<SecretInfo> = {
    label: (row) => row.name,
    hasItems: () => true,
    items: (row) => [
      {
        id: "set",
        label: row.hasValue ? "Replace value…" : "Set value…",
        onAction: () => setDialog({ kind: "set", name: row.name }),
      },
      {
        id: "delete",
        label: "Delete secret…",
        variant: "danger",
        onAction: () => setDialog({ kind: "delete", secret: row }),
      },
    ],
  };
  return (
    <Section
      title="Secrets"
      actions={
        secretsWritable ? (
          <Button onPress={() => setDialog({ kind: "set", name: null })}>Add secret</Button>
        ) : undefined
      }
    >
      <p className={styles.hint}>
        Passwords and keys are referred to by name in endpoint settings. Their values stay in the OS
        key store (or a restricted file) and are never shown.
      </p>
      {!secretsWritable && (
        <Alert tone="info" compact>
          {READ_ONLY_NOTE}
        </Alert>
      )}
      <QueryContent
        query={secrets}
        what="secrets"
        loadingRows={3}
        empty={{
          when: (data) => data.length === 0,
          title: "No secrets yet",
          description: secretsWritable
            ? "Add a secret for an endpoint that needs a password or key, such as SFTP."
            : "Secrets are set outside the console, with the OS key store or the secrets file.",
        }}
      >
        {(data) => (
          <DataGrid
            label="Secrets"
            columns={COLUMNS}
            rows={data}
            getRowId={(row) => row.name}
            onRowAction={
              secretsWritable ? (row) => setDialog({ kind: "set", name: row.name }) : undefined
            }
            rowActions={secretsWritable ? rowActions : undefined}
          />
        )}
      </QueryContent>
      {dialog?.kind === "set" && <SetSecretDialog existing={dialog.name} onClose={close} />}
      {dialog?.kind === "delete" && <DeleteSecretDialog secret={dialog.secret} onClose={close} />}
    </Section>
  );
}
