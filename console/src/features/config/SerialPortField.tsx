import { RefreshCw } from "lucide-react";
import { Controller, useFormContext } from "react-hook-form";
import { useParams } from "react-router";
import { useEngineQuery } from "@/api/hooks";
import { Alert } from "@/components/Alert";
import { Button } from "@/components/Button";
import type { EndpointFormValues } from "./endpointForm";
import { otherUses, portIdentity, SerialPortPicker } from "./SerialPortPicker";
import styles from "./SerialPortPicker.module.css";

const DESCRIPTION =
  "Choose a port found on the computer the engine runs on, or type a name such as COM12 or /dev/ttyUSB0.";

/**
 * The serial endpoint's port: a picker over the ports the engine lists, and what is known about
 * the one named. The value is the plain name; the list only helps to fill it in, so the form
 * works whether or not the engine can list the ports.
 */
export function SerialPortField() {
  const { control } = useFormContext<EndpointFormValues>();
  // Creating has no id in the address: every endpoint naming a port is another one.
  const { id: ownEndpointId } = useParams();
  const listing = useEngineQuery(["listSerialPorts"], (api) => api.listSerialPorts(), {
    staleTime: 0,
  });
  const ports = listing.data ?? [];
  const isLoading = listing.isPending;
  const hasFailed = listing.isError;
  return (
    <Controller
      control={control}
      name="port"
      rules={{
        validate: (value) => (value.trim() === "" ? "Enter port." : true),
      }}
      render={({ field, fieldState }) => {
        const named = ports.find((port) => port.name === field.value);
        const others = named ? otherUses(named, ownEndpointId) : [];
        return (
          <div className={styles.field}>
            <SerialPortPicker
              label="Port"
              name={field.name}
              ports={ports}
              ownEndpointId={ownEndpointId}
              value={field.value}
              onChange={field.onChange}
              inputRef={field.ref}
              isRequired
              isDisabled={isLoading}
              placeholder={isLoading ? "Looking for serial ports…" : "Choose a port or type a name"}
              description={DESCRIPTION}
              errorMessage={fieldState.error?.message}
            />
            {isLoading && (
              <p role="status" className={styles.status}>
                Looking for serial ports…
              </p>
            )}
            {hasFailed && (
              <Alert tone="warning" compact title="The engine could not list the serial ports">
                <p>You can still type the port name.</p>
                <Rescan
                  isPending={listing.isFetching}
                  label="Try again"
                  onPress={listing.refetch}
                />
              </Alert>
            )}
            {listing.isSuccess && (
              <div className={styles.status}>
                <p role="status">
                  {ports.length === 0
                    ? "No serial ports found on this computer. Type a name."
                    : `${ports.length} serial ${ports.length === 1 ? "port" : "ports"} found on this computer.`}
                </p>
                <Rescan isPending={listing.isFetching} label="Refresh" onPress={listing.refetch} />
              </div>
            )}
            {named && <p className={styles.status}>On this computer: {portIdentity(named)}.</p>}
            {others.length > 0 && (
              <Alert tone="warning" compact>
                {field.value} is already named by{" "}
                {others.map((use) => use.endpointLabel).join(", ")}. Two endpoints naming one port
                cannot both open it.
              </Alert>
            )}
            {listing.isSuccess && !named && field.value.trim() !== "" && (
              <Alert tone="info" compact>
                {field.value.trim()} was not found on this computer. It is kept as typed; the engine
                tries to open it when a channel using this endpoint starts.
              </Alert>
            )}
          </div>
        );
      }}
    />
  );
}

function Rescan({
  label,
  isPending,
  onPress,
}: {
  label: string;
  isPending: boolean;
  onPress: () => unknown;
}) {
  return (
    <Button variant="quiet" isPending={isPending} onPress={() => void onPress()}>
      <RefreshCw aria-hidden="true" />
      {label}
    </Button>
  );
}
