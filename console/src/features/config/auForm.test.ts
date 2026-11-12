import { describe, expect, it } from "vitest";
import type { AuSettings } from "@/api/types";
import {
  auToValues,
  controlBytes,
  emptyAu,
  etbAllowed,
  parseBytes,
  timeoutMs,
  valuesToAu,
} from "./auForm";

const SETTINGS: AuSettings = {
  profile: "au640",
  framingStart: [2],
  framingEnd: [13, 10],
  bcc: true,
  useEtb: false,
  maxWireLen: 512,
  framingClass: "b",
  unitNo: false,
  rackDigits: 4,
  sampleIdDigits: 10,
  sex: true,
  age: false,
  month: true,
  patientInfoDigits: [10, 0, 0, 0, 0, 0],
  resultDigits: 6,
  frameTimeoutMs: 1000,
  blockTimeoutMs: 3000,
};

describe("AU form values", () => {
  it("starts empty: nothing is chosen and nothing is preset", () => {
    const empty = emptyAu();
    expect(empty.profile).toBeNull();
    expect(empty.framingClass).toBeNull();
    expect(empty.maxWireLen).toBeNull();
    expect(empty.rackDigits).toBeNull();
    expect(empty.resultDigits).toBeNull();
    expect(empty.framingStart).toBe("");
    expect(empty.patientInfoDigits).toEqual(["", "", "", "", "", ""]);
    expect(valuesToAu(empty)).toBeUndefined();
  });

  it("round-trips the settings through the text the form holds", () => {
    const values = auToValues(SETTINGS);
    expect(values.framingEnd).toBe("13, 10");
    expect(values.maxWireLen).toBe("512");
    expect(valuesToAu(values)).toEqual(SETTINGS);
  });

  it("leaves a blank block timeout unset and shows an unset one as blank", () => {
    const { blockTimeoutMs: _set, ...unset } = SETTINGS;
    const values = auToValues(unset);
    expect(values.blockTimeoutMs).toBe("");
    expect(valuesToAu(values)).toEqual(unset);
    expect(valuesToAu({ ...values, blockTimeoutMs: "1200" })?.blockTimeoutMs).toBe(1200);
  });

  it("parses comma-separated control bytes", () => {
    expect(parseBytes("2")).toEqual([2]);
    expect(parseBytes(" 13 ,10")).toEqual([13, 10]);
    expect(parseBytes("")).toBeNull();
    expect(parseBytes("2,")).toBeNull();
    expect(parseBytes("2;3")).toBeNull();
    expect(parseBytes("-2")).toBeNull();
    expect(parseBytes("2.5")).toBeNull();
  });
});

describe("AU form checks", () => {
  it.each(["2", "3", "13, 10", "1,31"])("accepts control bytes %j", (text) => {
    expect(controlBytes(text)).toBe(true);
  });

  it.each([
    ["", /one or two whole numbers/],
    ["a", /one or two whole numbers/],
    ["1,2,3", /one or two whole numbers/],
    ["0", /1 to 31/],
    ["32", /1 to 31/],
    ["6", /ACK/],
    ["2, 21", /NAK/],
  ])("rejects control bytes %j", (text, message) => {
    expect(controlBytes(text)).toMatch(message);
  });

  it("allows ETB only when the end is the single byte 3", () => {
    expect(etbAllowed(false, "13, 10")).toBe(true);
    expect(etbAllowed(true, "3")).toBe(true);
    expect(etbAllowed(true, " 3 ")).toBe(true);
    expect(etbAllowed(true, "13, 10")).toMatch(/only be used when the end bytes are 3/);
    expect(etbAllowed(true, "3, 4")).toMatch(/only be used/);
    expect(etbAllowed(true, "")).toMatch(/only be used/);
  });

  it.each(["100", "1000", "9900"])("accepts the timeout %j", (text) => {
    expect(timeoutMs(text)).toBe(true);
  });

  it.each(["", "50", "0", "150", "9901", "10000", "abc", "1e3", "-100"])(
    "rejects the timeout %j",
    (text) => {
      expect(timeoutMs(text)).toMatch(/100 to 9900 milliseconds, in steps of 100/);
    },
  );
});
