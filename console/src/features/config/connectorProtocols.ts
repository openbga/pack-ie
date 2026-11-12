// Which protocols a connector can speak over which kind of endpoint (docs/spec/configuration.md
// §6.4). `raw` is what a bridge carries; message channels do not offer it.
import type { EndpointKind, Protocol } from "@/api/types";

export type MessageProtocol = Exclude<Protocol, "raw">;

const ENDPOINT_KINDS: Record<MessageProtocol, readonly EndpointKind[]> = {
  astm: ["serial", "tcp_client", "tcp_listener"],
  au: ["serial", "tcp_client", "tcp_listener"],
  hl7_mllp: ["tcp_client", "tcp_listener"],
  delimited_file: ["folder", "sftp"],
};

const ALL = Object.keys(ENDPOINT_KINDS) as MessageProtocol[];

/** The protocols that suit an endpoint kind; every one while no endpoint is chosen. */
export function protocolsFor(kind: EndpointKind | null): MessageProtocol[] {
  return kind === null ? ALL : ALL.filter((protocol) => ENDPOINT_KINDS[protocol].includes(kind));
}
