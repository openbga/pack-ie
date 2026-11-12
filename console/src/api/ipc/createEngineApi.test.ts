import { afterEach, describe, expect, it } from "vitest";
import { createEngineApi, selectEngine } from "../index";
import { MockEngineApi } from "../mock";
import { IpcEngineApi } from "./IpcEngineApi";

describe("selectEngine", () => {
  it.each([
    ["", true, "ipc"],
    ["", false, "mock"],
    ["?mock=1", true, "mock"],
    ["?mock=1", false, "mock"],
    ["?mock=offline", true, "mock-offline"],
    ["?mock=offline", false, "mock-offline"],
    ["?other=1", true, "ipc"],
    ["?mock=0", true, "ipc"],
  ] as const)("%j inside Tauri %s: %s", (search, insideTauri, expected) => {
    expect(selectEngine(search, insideTauri)).toBe(expected);
  });
});

describe("createEngineApi", () => {
  afterEach(() => {
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
    window.history.replaceState(null, "", "/");
  });

  it("uses the mock in a browser", () => {
    const api = createEngineApi();
    expect(api).toBeInstanceOf(MockEngineApi);
    (api as MockEngineApi).dispose();
  });

  it("uses the engine inside Tauri, unless the mock is asked for", () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
    expect(createEngineApi()).toBeInstanceOf(IpcEngineApi);
    window.history.replaceState(null, "", "/?mock=1");
    const mock = createEngineApi();
    expect(mock).toBeInstanceOf(MockEngineApi);
    (mock as MockEngineApi).dispose();
  });
});
