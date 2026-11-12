import { useEngineQuery } from "@/api/hooks";
import { Facts } from "@/components/Facts";
import { QueryContent } from "@/components/QueryContent";
import { Section } from "@/components/Section";

/** Versions of the engine and console, the build and the licence. */
export function AboutTab() {
  const info = useEngineQuery(["getEngineInfo"], (api) => api.getEngineInfo());
  return (
    <Section title="About Positive ACK">
      <QueryContent query={info} what="version information">
        {(data) => (
          <Facts
            items={[
              { label: "Engine version", value: data.engineVersion },
              { label: "Console version", value: data.consoleVersion },
              { label: "Protocol version", value: String(data.protocolVersion) },
              { label: "Build commit", value: data.buildCommit },
              { label: "Platform", value: data.platform },
              { label: "Licence", value: data.license },
            ]}
          />
        )}
      </QueryContent>
    </Section>
  );
}
