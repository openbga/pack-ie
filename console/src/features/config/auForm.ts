// The AU connector's form values and checks (docs/spec/configuration.md §7.2). There are no
// defaults: the laboratory copies the settings from the instrument's online parameter sheet, so
// a new connector starts empty. Numbers and control bytes are held as text while typed.
import type { AuSettings } from "@/api/types";

type Choice<Value> = Value | null;

export interface AuFormValues {
  profile: Choice<AuSettings["profile"]>;
  /** Comma-separated control bytes, such as `2` or `2, 3`. */
  framingStart: string;
  framingEnd: string;
  bcc: boolean;
  useEtb: boolean;
  /** `256`, `512` or `1024`. */
  maxWireLen: Choice<string>;
  framingClass: Choice<AuSettings["framingClass"]>;
  unitNo: boolean;
  /** `0`, `4` or `5`. */
  rackDigits: Choice<string>;
  sampleIdDigits: string;
  sex: boolean;
  age: boolean;
  month: boolean;
  /** Six digit counts. */
  patientInfoDigits: string[];
  /** `6` or `9`. */
  resultDigits: Choice<string>;
  /** `2` or `3`: the width of a test code. */
  testCodeDigits: "2" | "3";
  frameTimeoutMs: string;
  blockTimeoutMs: string;
}

export function emptyAu(): AuFormValues {
  return {
    profile: null,
    framingStart: "",
    framingEnd: "",
    bcc: false,
    useEtb: false,
    maxWireLen: null,
    framingClass: null,
    unitNo: false,
    rackDigits: null,
    sampleIdDigits: "",
    sex: false,
    age: false,
    month: false,
    patientInfoDigits: ["", "", "", "", "", ""],
    resultDigits: null,
    testCodeDigits: "2",
    frameTimeoutMs: "",
    blockTimeoutMs: "",
  };
}

export function auToValues(au: AuSettings): AuFormValues {
  return {
    ...au,
    framingStart: au.framingStart.join(", "),
    framingEnd: au.framingEnd.join(", "),
    maxWireLen: String(au.maxWireLen),
    rackDigits: String(au.rackDigits),
    sampleIdDigits: String(au.sampleIdDigits),
    patientInfoDigits: au.patientInfoDigits.map(String),
    resultDigits: String(au.resultDigits),
    testCodeDigits: au.testCodeDigits === 3 ? "3" : "2",
    frameTimeoutMs: String(au.frameTimeoutMs),
    blockTimeoutMs: au.blockTimeoutMs === undefined ? "" : String(au.blockTimeoutMs),
  };
}

/** The numbers in `1, 2`; null when anything in the text is not a whole number. */
export function parseBytes(text: string): number[] | null {
  const parts = text.split(",").map((part) => part.trim());
  return parts.every((part) => /^\d+$/.test(part)) ? parts.map(Number) : null;
}

/**
 * The AU settings the form holds, or undefined while a required choice is still open. The
 * form's own checks come first, so an open choice never reaches the engine as a guess.
 */
export function valuesToAu(values: AuFormValues): AuSettings | undefined {
  const { profile, framingClass, maxWireLen, rackDigits, resultDigits } = values;
  if (!profile || !framingClass || !maxWireLen || !rackDigits || !resultDigits) return undefined;
  return {
    profile,
    framingStart: parseBytes(values.framingStart) ?? [],
    framingEnd: parseBytes(values.framingEnd) ?? [],
    bcc: values.bcc,
    useEtb: values.useEtb,
    maxWireLen: Number(maxWireLen) as AuSettings["maxWireLen"],
    framingClass,
    unitNo: values.unitNo,
    rackDigits: Number(rackDigits) as AuSettings["rackDigits"],
    sampleIdDigits: Number(values.sampleIdDigits.trim()),
    sex: values.sex,
    age: values.age,
    month: values.month,
    patientInfoDigits: values.patientInfoDigits.map((digits) => Number(digits.trim())),
    resultDigits: Number(resultDigits) as AuSettings["resultDigits"],
    frameTimeoutMs: Number(values.frameTimeoutMs.trim()),
    // Blank leaves it to the engine's default (9900), so nothing is written for it.
    ...(values.blockTimeoutMs.trim() === ""
      ? {}
      : { blockTimeoutMs: Number(values.blockTimeoutMs.trim()) }),
    // The engine's default is 2, so only a 3 is written down.
    ...(values.testCodeDigits === "3" ? { testCodeDigits: 3 as const } : {}),
  };
}

const FORBIDDEN_BYTES = new Set([6, 21]);

/** What is wrong with a list of control bytes, or true: one or two, each 1 to 31, never ACK or NAK. */
export function controlBytes(text: string): true | string {
  const bytes = parseBytes(text);
  if (bytes === null || bytes.length > 2) {
    return "Enter one or two whole numbers separated by a comma, for example 2 or 13, 10.";
  }
  if (bytes.some((byte) => byte < 1 || byte > 31)) return "Each control byte is from 1 to 31.";
  if (bytes.some((byte) => FORBIDDEN_BYTES.has(byte))) {
    return "Control bytes may not be 6 (ACK) or 21 (NAK).";
  }
  return true;
}

/** ETB for intermediate blocks only works when the end is the single byte 3 (ETX). */
export function etbAllowed(useEtb: boolean, framingEnd: string): true | string {
  if (!useEtb) return true;
  const end = parseBytes(framingEnd);
  return (end?.length === 1 && end[0] === 3) || "ETB can only be used when the end bytes are 3.";
}

/** A timeout: 100 to 9900 milliseconds in steps of 100. */
export function timeoutMs(text: string): true | string {
  const valid = /^\d+$/.test(text) && Number(text) >= 100 && Number(text) <= 9900;
  return (valid && Number(text) % 100 === 0) || "Enter 100 to 9900 milliseconds, in steps of 100.";
}

export const WIRE_LENGTHS = ["256", "512", "1024"];
export const RACK_DIGITS = ["0", "4", "5"];
export const RESULT_DIGITS = ["6", "9"];
export const TEST_CODE_DIGITS = ["2", "3"];
