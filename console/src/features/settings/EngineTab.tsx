import type { ReactNode } from "react";
import { FormProvider, useForm } from "react-hook-form";
import { useEngineQuery } from "@/api/hooks";
import type { ConfigDocument, EngineSettings } from "@/api/types";
import { Alert } from "@/components/Alert";
import { Button } from "@/components/Button";
import { Facts } from "@/components/Facts";
import { FieldGrid, Form } from "@/components/Form";
import { TextInput } from "@/components/FormFields";
import { QueryContent } from "@/components/QueryContent";
import { Section } from "@/components/Section";
import { useToast } from "@/components/Toast";
import { appliedToast, useApplyDocument } from "@/features/config/applyDocument";
import { changeNoteValidator } from "@/features/config/changeNote";
import { wholeNumber } from "@/features/config/validators";
import { formatCount } from "@/lib/format";
import styles from "@/styles/layout.module.css";

const NOT_REPORTED = "Not reported by this engine";

/** A path or other literal in code type, or the note that the engine did not report it. */
function reported(value: string | null): ReactNode {
  return value === null ? NOT_REPORTED : <span className={styles.code}>{value}</span>;
}

function hostingLabel(hosting: EngineSettings["hosting"]): string {
  if (hosting === null) return NOT_REPORTED;
  return hosting === "service" ? "Operating system service" : "Console process";
}

function countOf(value: number | null, unit: string): string {
  return value === null ? NOT_REPORTED : `${formatCount(value)} ${unit}`;
}

/** The engine tells only its node id and name when the console cannot read its bootstrap file. */
function onlyIdentity(settings: EngineSettings): boolean {
  return [
    settings.stateDir,
    settings.logDir,
    settings.ipcPath,
    settings.hosting,
    settings.logRetentionDays,
    settings.logRingSize,
  ].every((value) => value === null);
}

function BootstrapSettings({ settings }: { settings: EngineSettings }) {
  return (
    <div className={styles.stack}>
      <Alert tone="info" compact>
        These settings are read when the engine starts; changing them needs a restart. To change
        them, edit bootstrap.json on the lab computer (or use the packie command line) and restart
        the engine.
      </Alert>
      {onlyIdentity(settings) && (
        <p className={styles.hint}>
          The engine's bootstrap file could not be read by the console, so only the node name and id
          are known.
        </p>
      )}
      <Facts
        items={[
          { label: "Node name", value: settings.nodeName },
          { label: "Node id", value: settings.nodeId },
          { label: "Hosting", value: hostingLabel(settings.hosting) },
          { label: "State directory", value: reported(settings.stateDir) },
          { label: "Log directory", value: reported(settings.logDir) },
          { label: "IPC path", value: reported(settings.ipcPath) },
          ...(settings.bootstrapPath
            ? [{ label: "Bootstrap file", value: reported(settings.bootstrapPath) }]
            : []),
          { label: "Log retention", value: countOf(settings.logRetentionDays, "days") },
          { label: "Log ring size", value: countOf(settings.logRingSize, "events") },
        ]}
      />
    </div>
  );
}

interface ThresholdValues {
  endpointDownSeconds: string;
  queueDepthWarning: string;
  bufferPressurePercent: string;
  diskFreeMinMib: string;
  changeNote: string;
}

const downSeconds = wholeNumber({ min: 1, max: 86_400 });
const queueDepth = wholeNumber({ min: 0, max: 10_000_000 });
const percent = wholeNumber({ min: 1, max: 100 });
const mebibytes = wholeNumber({ min: 1, max: 10_000_000 });

function toValues(doc: ConfigDocument): ThresholdValues {
  const { alertThresholds } = doc;
  return {
    endpointDownSeconds: String(alertThresholds.endpointDownSeconds),
    queueDepthWarning: String(alertThresholds.queueDepthWarning),
    bufferPressurePercent: String(alertThresholds.bufferPressurePercent),
    diskFreeMinMib: String(alertThresholds.diskFreeMinMib),
    changeNote: "",
  };
}

function ThresholdsForm({ doc, onReload }: { doc: ConfigDocument; onReload: () => void }) {
  const form = useForm<ThresholdValues>({ defaultValues: toValues(doc) });
  const toast = useToast();
  const apply = useApplyDocument();
  function save(values: ThresholdValues) {
    const document: ConfigDocument = {
      ...doc,
      alertThresholds: {
        endpointDownSeconds: Number(values.endpointDownSeconds),
        queueDepthWarning: Number(values.queueDepthWarning),
        bufferPressurePercent: Number(values.bufferPressurePercent),
        diskFreeMinMib: Number(values.diskFreeMinMib),
      },
    };
    apply.mutate(
      { document, note: values.changeNote },
      {
        onSuccess: (result) => {
          form.reset(values);
          toast.show(appliedToast(result, document.channels));
        },
      },
    );
  }
  return (
    <FormProvider {...form}>
      <Form
        aria-label="Alert thresholds"
        onSubmit={(event) => void form.handleSubmit(save)(event)}
        error={apply.error}
        errorTitle="The thresholds were not saved"
        fieldPaths={{ alertThresholds: "" }}
        actions={
          <>
            <Button type="submit" variant="primary" isPending={apply.isPending}>
              Save thresholds
            </Button>
            {apply.error?.code === "conflict" && (
              <Button
                onPress={() => {
                  apply.reset();
                  onReload();
                }}
              >
                Reload configuration
              </Button>
            )}
          </>
        }
      >
        <p className={styles.hint}>
          Saving applies a new configuration version. Thresholds belong to no channel, so no channel
          restarts.
        </p>
        <FieldGrid>
          <TextInput
            name="endpointDownSeconds"
            label="Endpoint down after (seconds)"
            isRequired
            validate={downSeconds}
            description="An alert opens when an enabled endpoint has been down this long."
          />
          <TextInput
            name="queueDepthWarning"
            label="Delivery queue warning (deliveries)"
            isRequired
            validate={queueDepth}
            description="An alert opens when a channel has this many deliveries waiting. 0 turns it off."
          />
          <TextInput
            name="bufferPressurePercent"
            label="Buffer pressure (percent)"
            isRequired
            validate={percent}
            description="An alert opens when a bridge buffer is this full."
          />
          <TextInput
            name="diskFreeMinMib"
            label="Disk free minimum (MiB)"
            isRequired
            validate={mebibytes}
            description="An alert opens when free disk space falls below this."
          />
          <TextInput
            name="changeNote"
            label="Change note"
            isRequired
            validate={changeNoteValidator}
            description="Why you are changing the thresholds, 1 to 500 characters. Recorded with the version and in the audit log."
          />
        </FieldGrid>
      </Form>
    </FormProvider>
  );
}

/** Bootstrap settings (read-only) and the alert thresholds (editable through apply). */
export function EngineTab() {
  const settings = useEngineQuery(["getEngineSettings"], (api) => api.getEngineSettings());
  const config = useEngineQuery(["getConfig"], (api) => api.getConfig());
  return (
    <div className={styles.stack}>
      <Section title="Engine">
        <QueryContent query={settings} what="engine settings">
          {(data) => <BootstrapSettings settings={data} />}
        </QueryContent>
      </Section>
      <Section title="Alert thresholds">
        <QueryContent query={config} what="alert thresholds">
          {(doc) => <ThresholdsForm doc={doc} onReload={() => void config.refetch()} />}
        </QueryContent>
      </Section>
    </div>
  );
}
