// The form's values for the protocol settings of a connector, and how they become the document's
// `ConnectorSettings` (docs/spec/configuration.md section 7). The form holds the settings of every
// protocol; the connector's protocol decides which are used. Numbers are held as text while typed.
// The engine's defaults are written here only as what a new connector shows.
import type { AckMode, ConnectorSettings, Protocol } from "@/api/types";
import { type AuFormValues, auToValues, emptyAu, valuesToAu } from "./auForm";

export interface SettingsFormValues {
  ackMode: AckMode;
  /** HL7: the version of generated messages. */
  hl7Version: string;
  requireEndCr: boolean;
  /** HL7 destination: the accepted answer codes, comma-separated. */
  ackMessageTypes: string;
  replyType: "ack" | "order_response";
  astmRole: "computer" | "instrument";
  maxFrameChars: string;
  enqAttempts: string;
  busyRetryMs: string;
  /** ASTM source: where the order record carries the sample id. */
  sampleIdPath: string;
  au: AuFormValues;
  /** The column template of a delimited connector, as JSON text. */
  templateJson: string;
}

/** `O.3`, `O.4.3`: the order record's field 3 to 99 and an optional component 1 to 99, no leading zeros. */
const SAMPLE_ID_PATH = /^O\.(?:[3-9]|[1-9]\d)(?:\.(?:[1-9]|[1-9]\d))?$/;

/** What is wrong with a sample id path, or true. */
export function sampleIdPathProblem(value: string): true | string {
  return (
    SAMPLE_ID_PATH.test(value.trim()) ||
    "Use O. and a field from 3 to 99, then optionally a component from 1 to 99, such as O.3 or O.4.3."
  );
}

export function emptySettings(): SettingsFormValues {
  return {
    ackMode: "original",
    hl7Version: "2.5.1",
    requireEndCr: true,
    ackMessageTypes: "ACK",
    replyType: "ack",
    astmRole: "computer",
    maxFrameChars: "240",
    enqAttempts: "3",
    busyRetryMs: "10000",
    sampleIdPath: "O.3",
    au: emptyAu(),
    templateJson: "",
  };
}

const text = (value: number | undefined, fallback: string) =>
  value === undefined ? fallback : String(value);

export function settingsToValues(settings: ConnectorSettings): SettingsFormValues {
  const defaults = emptySettings();
  return {
    ackMode: settings.ackMode ?? defaults.ackMode,
    hl7Version: settings.hl7Version ?? defaults.hl7Version,
    requireEndCr: settings.requireEndCr ?? defaults.requireEndCr,
    ackMessageTypes: settings.ackMessageTypes?.join(", ") ?? defaults.ackMessageTypes,
    replyType: settings.replyType ?? defaults.replyType,
    astmRole: settings.astmRole ?? defaults.astmRole,
    maxFrameChars: text(settings.maxFrameChars, defaults.maxFrameChars),
    enqAttempts: text(settings.enqAttempts, defaults.enqAttempts),
    busyRetryMs: text(settings.busyRetryMs, defaults.busyRetryMs),
    sampleIdPath: settings.sampleIdPath ?? defaults.sampleIdPath,
    au: settings.au ? auToValues(settings.au) : defaults.au,
    templateJson: settings.templateJson ?? defaults.templateJson,
  };
}

/** The codes in `ACK, ORL`: trimmed, without empty entries. */
export function parseCodes(value: string): string[] {
  return value
    .split(",")
    .map((code) => code.trim())
    .filter((code) => code !== "");
}

const whole = (value: string) => Number(value.trim());

/** The settings of the chosen protocol only; what was typed for another one is dropped. */
export function valuesToSettings(
  values: SettingsFormValues,
  protocol: Protocol | null,
): ConnectorSettings {
  switch (protocol) {
    case "hl7_mllp":
      return {
        ackMode: values.ackMode,
        hl7Version: values.hl7Version.trim(),
        requireEndCr: values.requireEndCr,
        ackMessageTypes: parseCodes(values.ackMessageTypes),
        replyType: values.replyType,
      };
    case "astm":
      return {
        astmRole: values.astmRole,
        maxFrameChars: whole(values.maxFrameChars),
        enqAttempts: whole(values.enqAttempts),
        busyRetryMs: whole(values.busyRetryMs),
        sampleIdPath: values.sampleIdPath.trim(),
      };
    case "au": {
      const au = valuesToAu(values.au);
      return au ? { au } : {};
    }
    case "delimited_file":
      return { templateJson: values.templateJson };
    default:
      return {};
  }
}
