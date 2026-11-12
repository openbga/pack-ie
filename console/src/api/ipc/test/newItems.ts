/** Builders for the items the `configNew` tests add to the rich document. */

import type {
  AuSettings,
  ChannelConfig,
  ConfigDocument,
  DestinationConfig,
  EndpointConfig,
  MessageChannelConfig,
} from "../../types";
import { documentFromWire, documentToWire } from "../configDocument";
import { IDS, richConfig } from "./configFixtures";

/** The new items `endpoints` and `channels` become when added to the rich document. */
export function withNew(endpoints: EndpointConfig[], channels: ChannelConfig[]) {
  const raw = richConfig();
  const doc: ConfigDocument = documentFromWire(raw);
  doc.endpoints.push(...endpoints);
  doc.channels.push(...channels);
  const converted = documentToWire(doc, raw);
  return {
    issues: converted.issues,
    endpoints: (converted.document.endpoints ?? []).slice(raw.endpoints?.length),
    channels: (converted.document.channels ?? []).slice(raw.channels?.length),
    document: converted.document,
  };
}

export const base = { id: "new-1", label: "New", enabled: true };

export const AU: AuSettings = {
  profile: "au400",
  framingStart: [2],
  framingEnd: [3],
  bcc: true,
  useEtb: false,
  maxWireLen: 256,
  framingClass: "a",
  unitNo: false,
  rackDigits: 0,
  sampleIdDigits: 6,
  sex: false,
  age: false,
  month: false,
  patientInfoDigits: [0, 0, 0, 0, 0, 0],
  resultDigits: 6,
  frameTimeoutMs: 1000,
  blockTimeoutMs: 1000,
};

export const destination = (extra: Partial<DestinationConfig> = {}): DestinationConfig => ({
  id: "d1",
  label: "LIS",
  endpointId: IDS.client,
  protocol: "hl7_mllp",
  settings: { ackMode: "original" },
  enabled: true,
  filters: [],
  mappings: [],
  codeTables: {},
  retry: { initialMs: 1000, maxMs: 60_000, maxAttempts: 5 },
  ...extra,
});

export const message = (extra: Partial<MessageChannelConfig> = {}): MessageChannelConfig => ({
  kind: "message",
  id: "new-m",
  label: "M",
  enabled: true,
  source: {
    id: "source",
    endpointId: IDS.listener,
    protocol: "astm",
    settings: { astmRole: "computer" },
    duplicateDetection: true,
  },
  filters: [],
  destinations: [destination()],
  retention: { days: 90, maxMessages: null },
  ...extra,
});

/** An SFTP endpoint with every setting at its default, signing in with a password. */
export const sftp = {
  ...base,
  kind: "sftp" as const,
  host: "h",
  port: 22,
  username: "u",
  authMethod: "password" as const,
  secretName: "pw",
  passphraseSecretName: "",
  hostKeyFingerprint: "SHA256:abcdefghij",
  remotePath: "/in",
  pollIntervalSec: 60,
  filePattern: "*",
} satisfies EndpointConfig;
