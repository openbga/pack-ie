/**
 * What `src/ipc/generated/` (the engine's ts-rs output) cannot say by itself. Snapshots, alerts,
 * events and answers are used exactly as generated. What stays here is small:
 *
 * - The runtime document the console READS and WRITES is looser than the generated
 *   `RuntimeConfig`. The engine always answers with a complete document, but it accepts (and the
 *   console writes) sparse ones: every field with a default in docs/spec/configuration.md may be
 *   left out, so new items stay sparse and the engine's defaults live in one place. The
 *   `Wire*` document types below are derived from the generated ones (`Sparse`, `Loose`), so a
 *   renamed or removed field breaks the build; the assertions at the end break it when a
 *   generated type no longer fits (a field made required here, a new kind).
 * - Four places are `unknown` or free JSON in the generated types and are read by their own
 *   modules: filter expressions (`filters.ts`), mapping rules (`mappings.ts`), protocol settings
 *   (`connectorConfig.ts`, `auConfig.ts`) and delimited-file settings (kept as JSON text).
 * - `AnyRequest` and `AnyResponse` swap the generated full document for the sparse one.
 */

import type {
  AlertThresholds,
  BridgeChannel,
  BufferPolicy,
  ChannelConfig,
  Connection,
  DeliveryOverrides,
  DeliveryPolicy,
  Destination,
  DiagnosticsOverrides,
  EndpointConfig,
  FolderSettings,
  Request,
  Response,
  Retention,
  RetryPolicy,
  RuntimeConfig,
  SecretRef,
  SerialSettings,
  SftpSettings,
  SourceConnector,
  Tap,
  TcpClientSettings,
  TcpListenerSettings,
} from "@/ipc/generated";

/** `T` with every field, at any depth, optional. Lists are kept whole. */
type Sparse<T> = T extends readonly unknown[]
  ? T
  : T extends object
    ? { [K in keyof T]?: Sparse<T[K]> }
    : T;

/** `T` with the fields `R` required (they have no default) and the rest sparse. */
type Loose<T, R extends keyof T> = Pick<T, R> & Sparse<Omit<T, R>>;

// ── Endpoints (configuration.md section 5) ─────────────────────────────────────────────────

export type WireSerialSettings = Loose<SerialSettings, "port_name" | "baud_rate">;
export type WireTcpClientSettings = Loose<TcpClientSettings, "host" | "port">;
export type WireTcpListenerSettings = Loose<TcpListenerSettings, "bind_address" | "port">;
export type WireFolderSettings = Loose<FolderSettings, "path">;

type WireSftpAuth =
  | { method: "password"; password: SecretRef }
  | { method: "key"; private_key: SecretRef; passphrase?: SecretRef | null };

export type WireSftpSettings = Loose<
  Omit<SftpSettings, "auth">,
  "host" | "username" | "host_key_fingerprint" | "remote_directory"
> & { auth: WireSftpAuth };

type WireEndpointBase = Loose<
  Pick<EndpointConfig, "endpoint_id" | "label" | "enabled" | "tags">,
  "endpoint_id" | "label"
>;

export type WireEndpoint = WireEndpointBase &
  (
    | { kind: "serial"; settings: WireSerialSettings }
    | { kind: "tcp_client"; settings: WireTcpClientSettings }
    | { kind: "tcp_listener"; settings: WireTcpListenerSettings }
    | { kind: "folder"; settings: WireFolderSettings }
    | { kind: "sftp"; settings: WireSftpSettings }
  );

// ── Channels (sections 6 and 7) ────────────────────────────────────────────────────────────

type WireRetry = Sparse<RetryPolicy>;
export type WireDelivery = Sparse<DeliveryPolicy>;
export type WireDeliveryOverrides = Sparse<DeliveryOverrides>;

/**
 * Protocol settings (section 7). Which keys exist depends on the protocol; the modules that
 * name keys check them against the generated settings types (`keyof AstmSettings`, ...).
 */
export type WireSettings = Record<string, unknown>;

/** A filter expression or mapping condition (section 8.1): free JSON, read by `filters.ts`. */
export type WireExpression = Record<string, unknown>;

/** A mapping rule (section 8.2): free JSON, read by `mappings.ts`. */
type WireRule = Record<string, unknown>;

export interface WireMapping {
  tables?: Record<string, Record<string, string>>;
  rules: WireRule[];
  require_single_group?: boolean;
  require?: Record<string, unknown>[];
}

export interface WireSource extends Pick<SourceConnector, "endpoint" | "protocol"> {
  settings?: WireSettings;
  duplicate_detection?: boolean;
}

export interface WireDestination
  extends Pick<Destination, "destination_id" | "label" | "endpoint" | "protocol"> {
  enabled?: boolean;
  settings?: WireSettings;
  filters?: WireExpression[];
  mapping?: WireMapping | null;
  delivery?: WireDeliveryOverrides;
}

type WireChannelBase = Loose<
  Pick<BridgeChannel, "channel_id" | "label" | "enabled" | "tags">,
  "channel_id" | "label"
>;

export type WireBridge = WireChannelBase &
  Loose<Pick<BridgeChannel, "endpoint_a" | "endpoint_b">, "endpoint_a" | "endpoint_b"> & {
    kind: "bridge";
    connection?: Sparse<Connection>;
    buffer?: Sparse<BufferPolicy>;
    retry?: WireRetry;
    tap?: Sparse<Tap>;
  };

export type WireMessage = WireChannelBase & {
  kind: "message";
  source: WireSource;
  filters?: WireExpression[];
  destinations: WireDestination[];
  retry?: WireRetry;
  delivery?: WireDelivery;
  retention?: Sparse<Retention>;
};

export type WireChannel = WireBridge | WireMessage;

export type WireAlerts = Sparse<AlertThresholds>;

/** `runtime.json` (section 4.1), as read and written by the console. */
export interface WireConfig {
  schema_version?: number;
  config_version?: number;
  generated_at?: string;
  diagnostics?: Sparse<DiagnosticsOverrides>;
  alerts?: WireAlerts;
  endpoints?: WireEndpoint[];
  channels?: WireChannel[];
}

// ── Requests and answers that carry the document ───────────────────────────────────────────

export type AnyRequest =
  | Exclude<Request, { type: "validate_config" | "put_config" }>
  | { type: "validate_config"; document: WireConfig }
  | { type: "put_config"; document: WireConfig; base_version: number; note: string | null };

export type AnyResponse =
  | Exclude<Response, { type: "config" }>
  | { type: "config"; document: WireConfig };

export type ResponseOf<T extends AnyResponse["type"]> = Extract<AnyResponse, { type: T }>;

// ── The generated types must still fit what is written above ───────────────────────────────

type Assert<T extends true> = T;
/** `filters` and `mapping` are `unknown` in the generated types; the console reads them as JSON. */
type FreeJson = "filters" | "mapping";
type Fits<G, W, Skip extends PropertyKey = never> =
  Omit<G, Skip> extends Omit<W, Skip> ? true : false;

/** Every generated document is a valid `WireConfig`: nothing became required, no kind is missing. */
export type GeneratedFitsEndpoint = Assert<EndpointConfig extends WireEndpoint ? true : false>;
export type GeneratedFitsAuth = Assert<SftpSettings["auth"] extends WireSftpAuth ? true : false>;
export type GeneratedFitsBridge = Assert<
  Fits<Extract<ChannelConfig, { kind: "bridge" }>, WireBridge>
>;
export type GeneratedFitsMessage = Assert<
  Fits<
    Extract<ChannelConfig, { kind: "message" }>,
    WireMessage,
    FreeJson | "source" | "destinations"
  >
>;
export type GeneratedFitsSource = Assert<Fits<SourceConnector, WireSource, "settings">>;
export type GeneratedFitsDestination = Assert<
  Fits<Destination, WireDestination, FreeJson | "settings">
>;
export type GeneratedFitsDocument = Assert<
  Fits<RuntimeConfig, WireConfig, "endpoints" | "channels">
>;
export type GeneratedFitsPut = Assert<
  Fits<
    Extract<Request, { type: "put_config" }>,
    Extract<AnyRequest, { type: "put_config" }>,
    "document"
  >
>;
export type GeneratedFitsValidate = Assert<
  Fits<
    Extract<Request, { type: "validate_config" }>,
    Extract<AnyRequest, { type: "validate_config" }>,
    "document"
  >
>;
