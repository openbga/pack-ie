import type { EndpointSnapshot } from "@/ipc/generated";
import type { WireEndpoint } from "./wire";

const PARITY_LETTERS = { none: "N", even: "E", odd: "O" } as const;

function hostPort(host: string, port: number): string {
  return `${host.includes(":") ? `[${host}]` : host}:${port}`;
}

/** One line on where an endpoint points, from its configuration. */
export function transportSummary(endpoint: WireEndpoint): string {
  switch (endpoint.kind) {
    case "serial": {
      const t = endpoint.settings;
      const parity = PARITY_LETTERS[t.parity ?? "none"];
      const stop = t.stop_bits === "two" ? 2 : 1;
      const port = t.virtual_link ? `virtual ${t.virtual_link}` : t.port_name;
      return `${port} · ${t.baud_rate} ${t.data_bits ?? 8}${parity}${stop}`;
    }
    case "tcp_client":
      return `tcp://${hostPort(endpoint.settings.host, endpoint.settings.port)}`;
    case "tcp_listener":
      return `listen ${hostPort(endpoint.settings.bind_address, endpoint.settings.port)}`;
    case "folder":
      return endpoint.settings.path;
    case "sftp": {
      const t = endpoint.settings;
      const directory = t.remote_directory.startsWith("/")
        ? t.remote_directory
        : `/${t.remote_directory}`;
      const port = (t.port ?? 22) === 22 ? "" : `:${t.port}`;
      return `sftp://${t.username}@${t.host}${port}${directory}`;
    }
  }
}

/**
 * Where an endpoint points: the engine's own non-secret description (`settings_summary`), else
 * the configured address, else the kind.
 */
export function endpointSummary(
  config: WireEndpoint | undefined,
  snapshot: Pick<EndpointSnapshot, "settings_summary" | "peer" | "kind">,
): string {
  if (snapshot.settings_summary) return snapshot.settings_summary;
  if (config !== undefined) return transportSummary(config);
  return snapshot.peer ?? snapshot.kind;
}
