import { describe, expect, it } from "vitest";
import { mockOptions, selectEngine } from "./index";

describe("selectEngine", () => {
  it("uses the real engine only inside Tauri, unless the mock is asked for", () => {
    expect(selectEngine("", true)).toBe("ipc");
    expect(selectEngine("?mock=1", true)).toBe("mock");
    expect(selectEngine("", false)).toBe("mock");
    expect(selectEngine("?mock=offline", true)).toBe("mock-offline");
  });
});

describe("mockOptions", () => {
  it("reads the mock's switches from the address", () => {
    expect(mockOptions("?mock=1")).toEqual({ offline: false });
    expect(mockOptions("?mock=1&messages=500000")).toMatchObject({ messageCount: 500_000 });
    expect(mockOptions("?mock=1&scenario=empty")).toMatchObject({ empty: true });
    expect(mockOptions("?mock=1&scenario=errors")).toMatchObject({ failReads: true });
    expect(mockOptions("?mock=1&scenario=slow")).toMatchObject({ latencyMs: 1500 });
    expect(mockOptions("?mock=offline")).toMatchObject({ offline: true });
    expect(mockOptions("?mock=1&steady=1")).toMatchObject({ steady: true });
  });

  it("ignores values it does not understand and caps the message count", () => {
    expect(mockOptions("?messages=abc&scenario=nope")).toEqual({ offline: false });
    expect(mockOptions("?messages=-5")).toEqual({ offline: false });
    expect(mockOptions("?messages=99999999").messageCount).toBe(2_000_000);
  });
});
