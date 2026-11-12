import { useEffect, useMemo, useState } from "react";
import type { UseFormReturn } from "react-hook-form";
import { useEngineMutation } from "@/api/hooks";
import type { EndpointConfig, EndpointKind, EngineError } from "@/api/types";
import { Button } from "@/components/Button";
import { FieldSection } from "@/components/Form";
import { ProbePanel } from "@/components/ProbeResult";
import { type EndpointFormValues, valuesToEndpoint } from "./endpointForm";

/** The engine names the endpoint it was given `endpoints[0]`; in the document it sits at `index`. */
function atIndex(error: EngineError | null, index: number): EngineError | null {
  if (error?.code !== "validation") return null;
  return {
    ...error,
    fieldErrors: error.fieldErrors.map((fieldError) => ({
      ...fieldError,
      field: fieldError.field.replace(/^endpoints\[0\]/, `endpoints[${index}]`),
    })),
  };
}

/**
 * A connection test of the form's current, unsaved settings. It reads the form and never writes
 * to it, so it does not make the form dirty. Settings the form's own rules refuse are marked on
 * its fields without asking the engine; settings the engine's rules refuse come back as
 * `rejection`, which the editor shows on the fields. Editing after a run makes the result stale.
 */
export function useConnectionTest(form: UseFormReturn<EndpointFormValues>, entityIndex: number) {
  const test = useEngineMutation(
    (api, endpoint: EndpointConfig) => api.testEndpoint({ endpoint }),
    {
      invalidates: ["getAuditEvents"],
    },
  );
  const [isIncomplete, setIncomplete] = useState(false);
  const [isStale, setStale] = useState(false);
  // Synchronizes with the form library, which lives outside React.
  useEffect(() => {
    const subscription = form.watch(() => setStale(true));
    return () => subscription.unsubscribe();
  }, [form]);
  const { error, reset, mutate } = test;
  const rejection = useMemo(() => atIndex(error, entityIndex), [error, entityIndex]);

  function run() {
    void form.handleSubmit(
      (values) => {
        const endpoint = valuesToEndpoint(values);
        setStale(false);
        setIncomplete(endpoint === null);
        if (endpoint === null) reset();
        else mutate(endpoint);
      },
      () => {
        setStale(false);
        setIncomplete(true);
        reset();
      },
    )();
  }

  return {
    run,
    rejection,
    isPending: test.isPending,
    report: test.data ?? null,
    error,
    isIncomplete: isIncomplete && !isStale,
    isStale,
  };
}

/** The "Test connection" section of the endpoint editor. */
export function TestConnection({
  test,
  kind,
}: {
  test: ReturnType<typeof useConnectionTest>;
  kind: EndpointKind;
}) {
  return (
    <FieldSection title="Test connection">
      <ProbePanel
        kind={kind}
        action={
          <Button isPending={test.isPending} onPress={test.run}>
            Test connection
          </Button>
        }
        isPending={test.isPending}
        report={test.report}
        error={test.error}
        isIncomplete={test.isIncomplete}
        isStale={test.isStale}
        isOnForm
        onRetry={test.run}
      />
    </FieldSection>
  );
}
