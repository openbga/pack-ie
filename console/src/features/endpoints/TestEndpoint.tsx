import { useEngineMutation, useEngineQuery } from "@/api/hooks";
import type { EndpointConfig, EndpointSnapshot } from "@/api/types";
import { Alert } from "@/components/Alert";
import { Button } from "@/components/Button";
import { ErrorState } from "@/components/ErrorState";
import { LoadingState } from "@/components/LoadingState";
import { HELD_PORT_HINT, ProbePanel } from "@/components/ProbeResult";
import { Section } from "@/components/Section";

function TestSaved({ endpoint, held }: { endpoint: EndpointConfig; held: boolean }) {
  const test = useEngineMutation(
    (api, config: EndpointConfig) => api.testEndpoint({ endpoint: config }),
    { invalidates: ["getAuditEvents"] },
  );
  const run = () => test.mutate(endpoint);
  return (
    <ProbePanel
      kind={endpoint.kind}
      action={
        <Button isPending={test.isPending} onPress={run}>
          Test connection
        </Button>
      }
      note={
        held && (
          <Alert tone="info" compact>
            {HELD_PORT_HINT}
          </Alert>
        )
      }
      isPending={test.isPending}
      report={test.data ?? null}
      error={test.error}
      onRetry={run}
    />
  );
}

/**
 * "Test connection" on the endpoint page: probes the endpoint's saved, active settings, as the
 * configuration document holds them (the endpoint's own detail leaves the definition out).
 */
export function TestEndpoint({ endpoint }: { endpoint: EndpointSnapshot }) {
  const config = useEngineQuery(["getConfig"], (api) => api.getConfig());
  const saved = config.data?.endpoints.find((each) => each.id === endpoint.id);
  const exclusive = endpoint.kind === "serial" || endpoint.kind === "tcp_listener";
  let body = <LoadingState label="Loading the saved settings…" />;
  if (config.isError) {
    body = (
      <ErrorState
        title="The saved settings could not be loaded"
        error={config.error}
        onRetry={() => void config.refetch()}
        isRetrying={config.isFetching}
      />
    );
  } else if (config.data && saved === undefined) {
    body = (
      <Alert tone="info">
        This endpoint is not in the active configuration, so it cannot be tested.
      </Alert>
    );
  } else if (saved) {
    body = <TestSaved endpoint={saved} held={exclusive && endpoint.heldBy !== null} />;
  }
  return <Section title="Test connection">{body}</Section>;
}
