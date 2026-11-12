import { useState } from "react";
import { useFormContext, useWatch } from "react-hook-form";
import { useEngineQuery } from "@/api/hooks";
import type { EndpointKind } from "@/api/types";
import { Alert } from "@/components/Alert";
import { FieldList } from "@/components/FieldList";
import { FieldGrid, FieldSection } from "@/components/Form";
import { ChoiceInput, TextInput } from "@/components/FormFields";
import { Select } from "@/components/Select";
import styles from "./config.module.css";
import type { EndpointFormValues } from "./endpointForm";
import { FormCheckbox } from "./FormCheckbox";
import { SerialPortField } from "./SerialPortField";
import {
  FILE_PATTERN,
  HOST_KEY_FINGERPRINT,
  PEER_ADDRESS,
  portNumber,
  wholeNumber,
} from "./validators";

const milliseconds = wholeNumber();
const seconds = wholeNumber();
const idleSeconds = wholeNumber({ min: 0, max: 604_800 });

const PARITY = [
  { id: "none", label: "None" },
  { id: "even", label: "Even" },
  { id: "odd", label: "Odd" },
];
const FLOW_CONTROL = [
  { id: "none", label: "None" },
  { id: "rts_cts", label: "Hardware (RTS/CTS)" },
  { id: "xon_xoff", label: "Software (XON/XOFF)" },
];
const DATA_BITS = ["5", "6", "7", "8"].map((bits) => ({ id: bits, label: bits }));
const STOP_BITS = ["1", "2"].map((bits) => ({ id: bits, label: bits }));
const AUTH_METHODS = [
  { id: "password", label: "Password" },
  { id: "private_key", label: "Private key" },
];
const FOLDER_MODES = [
  { id: "watch", label: "Watch for new files" },
  { id: "poll", label: "Poll on a schedule" },
];

function SerialFields() {
  return (
    <FieldSection title="Serial connection">
      <FieldGrid>
        <SerialPortField />
        <TextInput name="baudRate" label="Baud rate" isRequired validate={wholeNumber()} />
        <ChoiceInput name="dataBits" label="Data bits" options={DATA_BITS} isRequired />
        <ChoiceInput name="parity" label="Parity" options={PARITY} isRequired />
        <ChoiceInput name="stopBits" label="Stop bits" options={STOP_BITS} isRequired />
        <ChoiceInput name="flowControl" label="Flow control" options={FLOW_CONTROL} isRequired />
      </FieldGrid>
    </FieldSection>
  );
}

function TcpClientFields() {
  return (
    <FieldSection title="TCP connection (connects out)">
      <FieldGrid>
        <TextInput name="host" label="Host" isRequired description="Name or IP address." />
        <TextInput name="port" label="Port" isRequired validate={portNumber} />
        <TextInput
          name="connectTimeoutMs"
          label="Connect timeout (ms)"
          isRequired
          validate={milliseconds}
        />
        <IdleTimeout />
      </FieldGrid>
      <FormCheckbox name="keepalive" label="Send TCP keepalive probes" />
    </FieldSection>
  );
}

function IdleTimeout() {
  return (
    <TextInput
      name="idleTimeoutSeconds"
      label="Idle timeout (seconds)"
      isRequired
      validate={idleSeconds}
      description="Drops the connection after this long without data. 0 turns it off."
    />
  );
}

/** One address or range per entry, and the opt-out for an empty list (SEC-01). */
function PeerAllowlist() {
  const form = useFormContext<EndpointFormValues>();
  const entries = useWatch({ control: form.control, name: "allowlist" });
  const allowAny = useWatch({ control: form.control, name: "allowAnyPeer" });
  const update = (next: string[]) => form.setValue("allowlist", next, { shouldDirty: true });
  return (
    <>
      <FieldList
        name="allowlist"
        legend="Peer allowlist"
        emptyText="No entries."
        items={entries.map((_, index) => ({ id: String(index) }))}
        itemLabel={(index) => `Address ${index + 1}`}
        addLabel="Add address"
        onAdd={() => update([...entries, ""])}
        onRemove={(index) => update(entries.filter((_, at) => at !== index))}
        renderItem={(index) => (
          <TextInput
            name={`allowlist.${index}`}
            label="IP address or range"
            isRequired
            format={PEER_ADDRESS}
          />
        )}
      />
      <FormCheckbox name="allowAnyPeer" label="Allow any peer to connect" />
      <p className={styles.note}>
        With addresses listed, only those hosts may connect and this box is ignored. With none, a
        listener that is reachable from the network refuses to start unless this box is ticked; a
        listener bound to this computer only (127.0.0.1) needs neither.
      </p>
      {entries.length === 0 && (
        <Alert tone="warning" title="No peer allowlist">
          {allowAny
            ? "Any host that can reach this address and port may connect. Add the instrument or system addresses that should be allowed."
            : "Add the instrument or system addresses that may connect, or allow any peer. The engine rejects a network listener with neither."}
        </Alert>
      )}
    </>
  );
}

function TcpListenerFields() {
  return (
    <FieldSection title="TCP connection (listens)">
      <FieldGrid>
        <TextInput
          name="bindAddress"
          label="Bind address"
          isRequired
          description="The local address to listen on; 0.0.0.0 means every network interface."
        />
        <TextInput name="port" label="Port" isRequired validate={portNumber} />
        <IdleTimeout />
      </FieldGrid>
      <FormCheckbox name="keepalive" label="Send TCP keepalive probes" />
      <PeerAllowlist />
    </FieldSection>
  );
}

function FolderFields() {
  const form = useFormContext<EndpointFormValues>();
  const mode = useWatch({ control: form.control, name: "mode" });
  return (
    <FieldSection title="Folder">
      <FieldGrid>
        <TextInput
          name="path"
          label="Folder path"
          isRequired
          description="A local folder or a lab share."
        />
        <ChoiceInput name="mode" label="Mode" options={FOLDER_MODES} isRequired />
        {mode === "poll" && (
          <TextInput
            name="pollIntervalSec"
            label="Poll interval (seconds)"
            isRequired
            validate={seconds}
          />
        )}
        <TextInput name="filePattern" label="File name pattern" isRequired format={FILE_PATTERN} />
      </FieldGrid>
    </FieldSection>
  );
}

interface SecretNameProps {
  /** The form field holding the name. */
  name: "secretName" | "passphraseSecretName";
  label: string;
  description: string;
  isRequired?: boolean;
}

/** Pick an existing secret, or type a name that Settings › Secrets will hold a value for. */
function SecretName({ name, label, description, isRequired }: SecretNameProps) {
  const form = useFormContext<EndpointFormValues>();
  const secrets = useEngineQuery(["listSecrets"], (api) => api.listSecrets());
  const [picked, setPicked] = useState<string | null>(null);
  const names = secrets.data?.map((secret) => ({ id: secret.name, label: secret.name })) ?? [];
  return (
    <>
      <TextInput name={name} label={label} isRequired={isRequired} description={description} />
      {names.length > 0 && (
        <Select
          label={`Existing secrets for ${label.toLowerCase()}`}
          name={`existing-${name}`}
          options={names}
          value={picked}
          placeholder="Choose to fill in the name"
          onChange={(chosen) => {
            setPicked(chosen);
            if (chosen !== null) {
              form.setValue(name, chosen, { shouldDirty: true, shouldValidate: true });
            }
          }}
        />
      )}
    </>
  );
}

const STORED_SECRET_NOTE =
  "The value is stored under this name in Settings › Secrets, never in the configuration.";

function SftpAuthentication() {
  const form = useFormContext<EndpointFormValues>();
  const method = useWatch({ control: form.control, name: "authMethod" });
  const key = method === "private_key";
  return (
    <>
      <ChoiceInput name="authMethod" label="Authentication" options={AUTH_METHODS} isRequired />
      <SecretName
        name="secretName"
        label={key ? "Private key secret name" : "Password secret name"}
        description={`${key ? "The private key" : "The password"}. ${STORED_SECRET_NOTE}`}
        isRequired
      />
      {key && (
        <SecretName
          name="passphraseSecretName"
          label="Passphrase secret name"
          description="Only when the key has a passphrase; leave empty for none."
        />
      )}
    </>
  );
}

function SftpFields() {
  return (
    <FieldSection title="SFTP server">
      <FieldGrid>
        <TextInput name="host" label="Host" isRequired description="Name or IP address." />
        <TextInput name="port" label="Port" isRequired validate={portNumber} />
        <TextInput name="username" label="User name" isRequired />
        <SftpAuthentication />
        <TextInput
          name="hostKeyFingerprint"
          label="Host key fingerprint"
          isRequired
          format={HOST_KEY_FINGERPRINT}
        />
        <TextInput
          name="remotePath"
          label="Remote path"
          isRequired
          description="For example /outbound"
        />
        <TextInput
          name="pollIntervalSec"
          label="Poll interval (seconds)"
          isRequired
          validate={seconds}
        />
        <TextInput name="filePattern" label="File name pattern" isRequired format={FILE_PATTERN} />
      </FieldGrid>
    </FieldSection>
  );
}

/** The connection fields of one endpoint kind. */
export function ConnectionFields({ kind }: { kind: EndpointKind }) {
  switch (kind) {
    case "serial":
      return <SerialFields />;
    case "tcp_client":
      return <TcpClientFields />;
    case "tcp_listener":
      return <TcpListenerFields />;
    case "folder":
      return <FolderFields />;
    case "sftp":
      return <SftpFields />;
  }
}
