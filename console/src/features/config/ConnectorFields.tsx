import { useEffect, useRef } from "react";
import { type Control, type FieldValues, useFormContext, useWatch } from "react-hook-form";
import type { AckMode, EndpointKind, Protocol } from "@/api/types";
import { ChoiceInput, PickerInput, TextInput } from "@/components/FormFields";
import type { RecordOption } from "@/components/RecordPicker";
import { protocolLabel } from "@/lib/status";
import { AuFields } from "./AuFields";
import { passthroughProblem, STARTER_TEMPLATE } from "./channelForm";
import { protocolsFor } from "./connectorProtocols";
import { FormCheckbox } from "./FormCheckbox";
import { ConnectorPresets } from "./presets/ConnectorPresets";
import { parseCodes, sampleIdPathProblem } from "./settingsForm";
import { TemplateJsonField } from "./TemplateJsonField";
import { wholeNumber } from "./validators";

/** An endpoint to choose; `description` carries its kind and state. */
export interface EndpointOption extends RecordOption {
  kind: EndpointKind;
}

type ConnectorSide = "source" | "destination";

const LINK_ROLES = [
  { id: "computer", label: "Computer" },
  { id: "instrument", label: "Instrument" },
];

/** A source must acknowledge; only a destination may expect no acknowledgement. */
function ackOptions(side: ConnectorSide, current: AckMode | null) {
  const options = [
    { id: "original", label: "Original" },
    { id: "enhanced", label: "Enhanced" },
  ];
  if (side === "destination") options.push({ id: "none", label: "None" });
  else if (current === "none")
    options.push({ id: "none", label: "None (not allowed for a source)" });
  return options;
}

const REPLY_TYPES = [
  { id: "ack", label: "An ACK for every message" },
  { id: "order_response", label: "An order response (ORL, ORR) to order messages" },
];

/** A list of message codes: one to eight, each three capitals or digits. */
function codesProblem(value: string): true | string {
  const codes = parseCodes(value);
  if (codes.length < 1 || codes.length > 8) return "Enter one to eight codes, separated by commas.";
  return (
    codes.every((code) => /^[A-Z0-9]{3}$/.test(code)) ||
    "Each code is three capital letters or digits, such as ACK or ORL."
  );
}

function Hl7Settings({ prefix, side }: { prefix: string; side: ConnectorSide }) {
  const { control } = useFormContext();
  const name = `${prefix}.settings.ackMode`;
  const current = useWatch({ control, name }) as AckMode | null;
  return (
    <>
      <Hl7AckMode name={name} side={side} current={current} />
      <TextInput
        name={`${prefix}.settings.hl7Version`}
        label="HL7 version"
        isRequired
        description="The version of the messages PACK-IE writes (MSH-12), such as 2.5.1. Received messages keep their own."
        format={{
          pattern: /^2\.\d+(\.\d+)?$/,
          message: "Enter a version such as 2.3, 2.5 or 2.5.1.",
          description: "",
        }}
      />
      <FormCheckbox
        name={`${prefix}.settings.requireEndCr`}
        label="Carriage return after the end block"
        description="Ticked: a frame ends with the end byte and a carriage return. Clear it for senders that end a frame with the end byte alone; a carriage return that does follow is accepted. Frames PACK-IE writes always end with both."
      />
      {side === "destination" ? (
        <TextInput
          name={`${prefix}.settings.ackMessageTypes`}
          label="Accepted answer codes"
          isRequired
          description="The message codes (MSH-9.1) accepted as the answer to a sent message, separated by commas. An order message is answered by an order response: add ORL or ORR, or the answer is ignored and the delivery times out."
          validate={codesProblem}
        />
      ) : (
        <ChoiceInput
          name={`${prefix}.settings.replyType`}
          label="Reply with"
          options={REPLY_TYPES}
          isRequired
          description="What this source answers with. Order messages are OML^O33, OML^O35, OML^O21 and ORM^O01; every other message still gets an ACK."
        />
      )}
    </>
  );
}

function Hl7AckMode({
  name,
  side,
  current,
}: {
  name: string;
  side: ConnectorSide;
  current: AckMode | null;
}) {
  return (
    <ChoiceInput
      name={name}
      label="Acknowledgement mode"
      options={ackOptions(side, current)}
      isRequired
      description={
        side === "destination"
          ? "Original or enhanced: the acknowledgement expected from the receiver. None: expect no acknowledgement; delivery counts as done once sent."
          : "Original acknowledges each message once stored; enhanced adds the application acknowledgement."
      }
    />
  );
}

function AstmSettings({ prefix, side }: { prefix: string; side: ConnectorSide }) {
  return (
    <>
      <ChoiceInput
        name={`${prefix}.settings.astmRole`}
        label="Link role"
        options={LINK_ROLES}
        isRequired
        description="Who gives way when both ends start sending at once: the computer yields and sends after receiving, the instrument has priority."
      />
      <TextInput
        name={`${prefix}.settings.maxFrameChars`}
        label="Characters per frame"
        isRequired
        validate={wholeNumber({ min: 64, max: 64_000 })}
        description="64 to 64000. 240 is the standard limit; larger frames are for instruments that document long frames over TCP. A longer record is split, and both ends must agree."
      />
      {side === "source" && (
        <TextInput
          name={`${prefix}.settings.sampleIdPath`}
          label="Sample id path"
          isRequired
          validate={sampleIdPathProblem}
          description="Where the order record (O) carries the sample id that Messages search by: O.3 is field 3, O.4.3 is component 3 of field 4. Field 3 to 99, component 1 to 99, no zeros in front. Instruments that put the specimen id in field 4 need O.4 or O.4.3."
        />
      )}
      {side === "destination" && (
        <>
          <TextInput
            name={`${prefix}.settings.enqAttempts`}
            label="ENQ attempts"
            isRequired
            validate={wholeNumber({ min: 1, max: 10 })}
            description="1 to 10. How many times PACK-IE asks to send a message before giving up; the delivery policy then retries. Some instruments document 7."
          />
          <TextInput
            name={`${prefix}.settings.busyRetryMs`}
            label="Wait after a busy answer (ms)"
            isRequired
            validate={wholeNumber({ min: 1000, max: 60_000 })}
            description="1000 to 60000. The wait after a NAK, or silence, before asking again."
          />
        </>
      )}
    </>
  );
}

function ProtocolSettings({
  prefix,
  protocol,
  side,
}: {
  prefix: string;
  protocol: Protocol | null;
  side: ConnectorSide;
}) {
  switch (protocol) {
    case "hl7_mllp":
      return <Hl7Settings prefix={prefix} side={side} />;
    case "astm":
      return <AstmSettings prefix={prefix} side={side} />;
    case "au":
      return <AuFields prefix={`${prefix}.settings.au`} />;
    case "delimited_file":
      return <TemplateJsonField name={`${prefix}.settings.templateJson`} />;
    default:
      return null;
  }
}

interface ConnectorFieldsProps {
  /** The form path of the connector: `source` or `destinations.0`. */
  prefix: string;
  endpoints: readonly EndpointOption[];
  side: ConnectorSide;
  /** A destination's position, which is how its mapping is found to check pass-through. */
  destinationIndex?: number;
}

const hasEntries = (entries: readonly unknown[] | undefined) => (entries?.length ?? 0) > 0;

/** Whether a destination has mapping rules or code tables; false for anything else. */
function useHasMapping(control: Control<FieldValues>, prefix: string): boolean {
  const rules = useWatch({ control, name: `${prefix}.mappings`, compute: hasEntries });
  const tables = useWatch({ control, name: `${prefix}.codeTables`, compute: hasEntries });
  const hidden = useWatch({ control, name: `${prefix}.mappingHidden` }) === true;
  return rules || tables || hidden;
}

/**
 * Shows the pass-through rule as soon as it is broken or mended: when the source protocol,
 * this destination's protocol or whether it has a mapping changes, the protocol field is
 * checked again (the form library would otherwise check it only when it is edited or submitted).
 */
function useRecheckPassthrough(
  protocolName: string,
  protocol: Protocol | null,
  isDestination: boolean,
) {
  const { control, trigger } = useFormContext();
  const sourceProtocol = useWatch({ control, name: "source.protocol" }) as Protocol | null;
  const mapped = useHasMapping(control, protocolName.replace(/\.protocol$/, ""));
  const key = `${sourceProtocol}|${protocol}|${mapped}`;
  const checked = useRef(key);
  useEffect(() => {
    if (checked.current === key) return;
    checked.current = key;
    // A protocol still to be chosen is reported by Validate, not by editing something else.
    if (isDestination && protocol !== null) void trigger(protocolName);
  }, [key, isDestination, protocol, protocolName, trigger]);
}

/** One connector: its endpoint, a protocol that suits it, and the protocol's settings. */
export function ConnectorFields({
  prefix,
  endpoints,
  side,
  destinationIndex,
}: ConnectorFieldsProps) {
  const { control, getValues, setValue } = useFormContext();
  const protocol = useWatch({ control, name: `${prefix}.protocol` }) as Protocol | null;
  const endpointId = useWatch({ control, name: `${prefix}.endpointId` }) as string | null;
  const kind = endpoints.find((endpoint) => endpoint.id === endpointId)?.kind ?? null;
  const allowed: Protocol[] = protocolsFor(kind);
  const options = allowed.map((id) => ({ id, label: protocolLabel(id) }));
  // A protocol the endpoint no longer suits stays visible, so the error has something to name.
  if (protocol && !allowed.includes(protocol)) {
    options.push({
      id: protocol,
      label: `${protocolLabel(protocol)} (does not suit this endpoint)`,
    });
  }
  const templateName = `${prefix}.settings.templateJson`;
  useRecheckPassthrough(`${prefix}.protocol`, protocol, side === "destination");
  return (
    <>
      <PickerInput name={`${prefix}.endpointId`} label="Endpoint" options={endpoints} isRequired />
      <ChoiceInput
        name={`${prefix}.protocol`}
        label="Protocol"
        options={options}
        isRequired
        description="Only protocols that suit the kind of endpoint are listed."
        validate={(chosen, values) => {
          if (chosen === null) return true;
          if (!allowed.includes(chosen as Protocol)) {
            return `${protocolLabel(chosen as Protocol)} cannot be used with this kind of endpoint.`;
          }
          const destination =
            destinationIndex === undefined ? undefined : values.destinations?.[destinationIndex];
          return (
            (destination && passthroughProblem(values.source?.protocol ?? null, destination)) ||
            true
          );
        }}
        onChange={(chosen) => {
          const blank = String(getValues(templateName) ?? "").trim() === "";
          if (chosen === "delimited_file" && blank) setValue(templateName, STARTER_TEMPLATE);
        }}
      />
      <ProtocolSettings prefix={prefix} protocol={protocol} side={side} />
      <ConnectorPresets prefix={prefix} protocol={protocol} side={side} />
    </>
  );
}
