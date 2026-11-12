import type { Protocol } from "../types";
import { PANELS, type PanelKey, resultFor } from "./panels";
import { hash32, pickBy } from "./prng";
import { compactTime, pad } from "./util";

export type BodyKind =
  | "hl7_oru"
  | "hl7_orm"
  | "hl7_adt"
  | "astm_result"
  | "au_result"
  | "csv_row"
  | "csv_order";

export interface BodyInput {
  kind: BodyKind;
  panel: PanelKey;
  seed: number;
  /** The identity index the message's content derives from. */
  index: number;
  /** Results (or orders) in the message. */
  n: number;
  sender: string;
  control: string;
  sample: string;
  patientNum: number;
  patient: string;
  accession: string;
  atMs: number;
  /** Exact size in bytes to pad the original body to; 0 for natural size. */
  target: number;
}

export const STX = "\x02";
const ETX = "\x03";
export const EOT = "\x04";
export const ENQ = "\x05";
export const ACK = "\x06";
export const NAK = "\x15";
export const SB = "\x0b";
export const EB = "\x1c";

const LAST = ["TEST", "SAMPLE", "DEMO", "MOCK", "SYNTH", "DUMMY"];
const FIRST = ["PATIENT", "PERSON", "SUBJECT", "CASE", "ALPHA", "BRAVO"];
const SENTENCE = "Specimen received and processed in the routine workflow. ";

function patientName(patientNum: number): string {
  const last = pickBy(LAST, patientNum);
  const first = pickBy(FIRST, Math.floor(patientNum / 6));
  return `${last}^${first}`;
}

function birthDate(patientNum: number): string {
  return `${1940 + (patientNum % 60)}${pad(1 + (patientNum % 12), 2)}${pad(1 + (patientNum % 28), 2)}`;
}

function sex(patientNum: number): string {
  return pickBy(["F", "M", "U"], patientNum >> 2);
}

function filler(length: number): string {
  return SENTENCE.repeat(Math.ceil(length / SENTENCE.length) + 1).slice(0, length);
}

/** Builds the text with an empty filler, then again padded so the result has exactly `target` bytes. */
function fitTo(build: (fill: string) => string, target: number): string {
  const natural = build("");
  return target > natural.length ? build(filler(target - natural.length)) : natural;
}

function checksum(body: string): string {
  let sum = 0;
  for (let i = 0; i < body.length; i++) sum = (sum + body.charCodeAt(i)) % 256;
  return sum.toString(16).toUpperCase().padStart(2, "0");
}

/** ASTM E1381 frames: STX, frame number, record, CR, ETX, two checksum digits, CR, LF. */
function astmFrames(records: string[]): string {
  return records
    .map((record, index) => {
      const body = `${(index + 1) % 8}${record}\r${ETX}`;
      return `${STX}${body}${checksum(body)}\r\n`;
    })
    .join("");
}

function results(input: BodyInput) {
  return Array.from({ length: input.n }, (_, k) =>
    resultFor(input.panel, k, hash32(input.seed, input.index, 100 + k)),
  );
}

/** MSH segment; `from` and `to` are `application|facility` pairs. */
function hl7Header(input: BodyInput, type: string, from: string, to: string): string {
  return `MSH|^~\\&|${from}|${to}|${compactTime(input.atMs)}||${type}|${input.control}|P|2.5.1`;
}

function astmPatient(input: BodyInput): string {
  const p = input.patientNum;
  return `P|1||${input.patient}||${patientName(p)}||${birthDate(p)}|${sex(p)}`;
}

function hl7Pid(input: BodyInput): string {
  const p = input.patientNum;
  return `PID|1||${input.patient}^^^MRN||${patientName(p)}||${birthDate(p)}|${sex(p)}`;
}

function hl7Oru(input: BodyInput, sending = input.sender): string {
  const collected = compactTime(input.atMs - 20 * 60_000);
  const panel = PANELS[input.panel];
  const build = (fill: string): string => {
    const obx = results(input).map(
      (r, k) =>
        `OBX|${k + 1}|NM|${r.test.code}^${r.test.name}||${r.text}|${r.test.unit}|${r.range}|${r.flag}|||F`,
    );
    return [
      hl7Header(input, "ORU^R01", `${sending}|MAINLAB`, "LIS|HOSP"),
      hl7Pid(input),
      `OBR|1|${input.accession}|${input.sample}|${panel.analyzer}^${panel.analyzer} panel|||${collected}`,
      ...obx,
      `NTE|1||${fill}`,
    ].join("\r");
  };
  return fitTo(build, input.target);
}

function hl7Orm(input: BodyInput, transformed = false): string {
  const ts = compactTime(input.atMs);
  const tests = PANELS.chem.tests;
  const build = (fill: string): string => {
    const orders = Array.from({ length: input.n }, (_, k) => {
      const test = pickBy(tests, hash32(input.seed, input.index, 200 + k));
      return [
        `ORC|NW|${input.accession}-${k + 1}|||||||${ts}`,
        `OBR|${k + 1}|${input.accession}-${k + 1}|${input.sample}|${test.code}^${test.name}|||${ts}`,
      ];
    }).flat();
    return [
      transformed
        ? hl7Header(input, "ORM^O01", "PACKIE|MAINLAB", "LIS|HOSP")
        : hl7Header(input, "ORM^O01", "LIS|HOSP", "PACKIE|MAINLAB"),
      hl7Pid(input),
      ...orders,
      `NTE|1||${fill}`,
    ].join("\r");
  };
  return fitTo(build, input.target);
}

function hl7Adt(input: BodyInput): string {
  const ts = compactTime(input.atMs);
  const build = (fill: string): string =>
    [
      hl7Header(input, "ADT^A08", "ADT|HOSP", "PACKIE|MAINLAB"),
      `EVN|A08|${ts}`,
      hl7Pid(input),
      `PV1|1|O|CLINIC^^^MAIN||||^TEST^DOCTOR|||||||||||V${pad(input.index % 1_000_000, 6)}`,
      `NTE|1||${fill}`,
    ].join("\r");
  return fitTo(build, input.target);
}

function astmResult(input: BodyInput): string {
  const ts = compactTime(input.atMs);
  const build = (fill: string): string => {
    const rs = results(input);
    const codes = rs.map((r) => `^^^${r.test.code}`).join("\\");
    const records = [
      `H|\\^&|||${input.sender}^1.0|||||LIS||P|1|${ts}`,
      astmPatient(input),
      `O|1|${input.sample}||${codes}|R|${ts}||||||||SER`,
      ...rs.map(
        (r, k) =>
          `R|${k + 1}|^^^${r.test.code}|${r.text}|${r.test.unit}|${r.range}|${r.flag}||F||||${ts}`,
      ),
      `C|1|I|${fill}|G`,
      "L|1|N",
    ];
    return astmFrames(records);
  };
  return fitTo(build, input.target);
}

function fixed(text: string, width = 60): string {
  return text.padEnd(width).slice(0, width);
}

/** AU fixed-width records: 60 characters per line, so the size follows from the line count. */
function auResult(input: BodyInput, header = "H"): string {
  const ts = compactTime(input.atMs);
  const order = header === "O";
  const lines = [
    fixed(`${header} 01 ${ts} ${input.sender.slice(0, 10).padEnd(10)}`),
    ...results(input).map((r, k) =>
      fixed(
        `${order ? "O" : "R"} ${pad(1 + (input.index % 9999), 4)} ${pad(1 + k, 2)} ${input.sample.padEnd(13)} ${r.test.code.padEnd(5)} ${(order ? "" : r.text).padStart(9)} ${(order ? "" : r.test.unit).padEnd(8)} ${order ? "" : r.flag}`,
      ),
    ),
    fixed(`E ${pad(input.n, 3)}`),
  ];
  return `${STX}${lines.map((line) => `${line}\r\n`).join("")}${ETX}`;
}

function csvRow(input: BodyInput): string {
  const collected = new Date(input.atMs - 20 * 60_000).toISOString().slice(0, 19).replace("T", " ");
  const r = results({ ...input, n: 1 })[0];
  const build = (fill: string): string =>
    [
      "sample_id,patient_id,test,value,unit,flag,collected_at,comment",
      `${input.sample},${input.patient},${r?.test.code ?? ""},${r?.text ?? ""},${r?.test.unit ?? ""},${r?.flag ?? ""},${collected},${fill}`,
    ]
      .map((line) => `${line}\r\n`)
      .join("");
  return fitTo(build, input.target);
}

function csvOrder(input: BodyInput): string {
  const ordered = new Date(input.atMs).toISOString().slice(0, 19).replace("T", " ");
  const build = (fill: string): string => {
    const tests = Array.from(
      { length: Math.min(input.n, 4) },
      (_, k) => pickBy(PANELS.chem.tests, hash32(input.seed, input.index, 200 + k)).code,
    ).join(";");
    return [
      "sample_id,patient_id,tests,priority,ordered_at,comment",
      `${input.sample},${input.patient},${tests},R,${ordered},${fill}`,
    ]
      .map((line) => `${line}\r\n`)
      .join("");
  };
  return fitTo(build, input.target);
}

/** The message as the source sent it. */
export function buildOriginal(input: BodyInput): string {
  switch (input.kind) {
    case "hl7_oru":
      return hl7Oru(input);
    case "hl7_orm":
      return hl7Orm(input);
    case "hl7_adt":
      return hl7Adt(input);
    case "astm_result":
      return astmResult(input);
    case "au_result":
      return auResult(input);
    case "csv_row":
      return csvRow(input);
    case "csv_order":
      return csvOrder(input);
  }
}

/** The exact size of `buildOriginal` for a kind, derived cheaply from the result count. */
export function targetSize(kind: BodyKind, n: number, hash: number): number {
  switch (kind) {
    case "hl7_oru":
      return 275 + 55 * n + (hash % 40);
    case "hl7_orm":
      return 200 + 112 * n + (hash % 40);
    case "hl7_adt":
      return 250 + (hash % 48);
    case "astm_result":
      return 225 + 66 * n + (hash % 40);
    case "au_result":
      return 62 * (n + 2) + 2;
    case "csv_row":
      return 150 + (hash % 60);
    case "csv_order":
      return 130 + (hash % 60);
  }
}

/** What the destination receives after the channel's mappings, in the destination's protocol. */
export function buildTransformed(input: BodyInput, destination: string): string {
  const plain = { ...input, target: 0 };
  const order = input.panel === "order";
  if (destination === "hl7_mllp") return order ? hl7Orm(plain, true) : hl7Oru(plain, "PACKIE");
  if (destination === "astm") return astmResult(plain);
  if (destination === "au") return auResult(plain, order ? "O" : "H");
  return order ? csvOrder(plain) : csvRow(plain);
}

/**
 * A message whose bytes are complete but do not parse (state `invalid`): not a message of its
 * protocol, or one without its required header fields.
 */
export function buildInvalid(input: BodyInput, protocol: Protocol): string {
  const ts = compactTime(input.atMs);
  const variant = hash32(input.seed, input.index, 30) % 2;
  switch (protocol) {
    case "hl7_mllp":
      return variant === 0
        ? `ZZZ|this is not an HL7 message|${pad(input.index % 100_000, 5)}`
        : `MSH|^~\\&|${input.sender}|MAINLAB|||${ts}\rPID|1||`;
    case "astm":
      return astmFrames([`H|\\^&|||${input.sender}^1.0`, "X|1|not an ASTM record", "L|1|N"]);
    case "au":
      return `${STX}${fixed("Z unknown record")}\r\n${ETX}`;
    default:
      return "sample_id,patient_id\r\n,\r\n,,,,\r\n";
  }
}

/** The frames received before a transfer ended without its last record (state `aborted`). */
export function buildAborted(input: BodyInput, protocol: Protocol): string {
  const ts = compactTime(input.atMs);
  if (protocol === "au") {
    const head = fixed(`H 01 ${ts} ${input.sender.slice(0, 10).padEnd(10)}`);
    return `${STX}${head}\r\n`;
  }
  return astmFrames([
    `H|\\^&|||${input.sender}^1.0|||||LIS||P|1|${ts}`,
    astmPatient(input),
    `O|1|${input.sample}||^^^GLU|R|${ts}`,
  ]);
}
