import { describe, expect, it } from "vitest";
import type { EndpointConfig, EndpointKind, ProbeReport } from "../types";
import type { MockEngineApi } from "./index";
import { mockApiFactory, rejection } from "./mockTestSupport";

const create = mockApiFactory({ messageCount: 500, seed: 7 });

async function endpointOf<K extends EndpointKind>(
  api: MockEngineApi,
  id: string,
  kind: K,
  changes: Partial<Extract<EndpointConfig, { kind: K }>> = {},
): Promise<Extract<EndpointConfig, { kind: K }>> {
  const found = (await api.getConfig()).endpoints.find((each) => each.id === id);
  if (found?.kind !== kind) throw new Error(`no ${kind} endpoint ${id}`);
  return { ...found, ...changes } as Extract<EndpointConfig, { kind: K }>;
}

const names = (report: ProbeReport) => report.steps.map((step) => step.name);
const failedStep = (report: ProbeReport) => report.steps.find((step) => !step.ok)?.name;

describe("testEndpoint (mock)", () => {
  it("probes a healthy TCP client: resolve, connect with its time, nothing after", async () => {
    const api = create();
    const endpoint = await endpointOf(api, "ep-lis-out", "tcp_client");
    const report = await api.testEndpoint({ endpoint });
    expect(report.ok).toBe(true);
    expect(names(report)).toEqual(["resolve", "connect"]);
    expect(report.steps[1]?.detail).toMatch(/^connected to .+ in \d+ ms, then closed$/);
    expect(report.summary).toBe(
      `lis.lab.local:2575 accepted a connection in ${report.steps[1]?.millis} ms.`,
    );
    expect(await api.testEndpoint({ endpoint })).toEqual(report);
  });

  it.each([
    ["nosuchhost.invalid", 2575, "resolve", "host name did not resolve"],
    ["unreachable.example", 2575, "connect", "connection timed out"],
    ["lis.lab.local", 9, "connect", "connection refused"],
  ])("fails a TCP client to %s:%s at the %s step", async (host, port, step, cause) => {
    const api = create();
    const endpoint = await endpointOf(api, "ep-lis-out", "tcp_client", { host, port });
    const report = await api.testEndpoint({ endpoint });
    expect(report.ok).toBe(false);
    expect(failedStep(report)).toBe(step);
    expect(report.steps.at(-1)?.detail).toBe(cause);
    expect(report.summary).toBe(`The ${step} step failed: ${cause}.`);
  });

  it("times out a connect after the endpoint's own limit, within the overall one", async () => {
    const api = create();
    const endpoint = await endpointOf(api, "ep-lis-out", "tcp_client", {
      host: "unreachable.example",
      connectTimeoutMs: 5000,
    });
    expect((await api.testEndpoint({ endpoint })).steps.at(-1)?.millis).toBe(5000);
    expect((await api.testEndpoint({ endpoint, timeoutSeconds: 2 })).steps.at(-1)?.millis).toBe(
      2000,
    );
  });

  it("binds and releases a free listener port", async () => {
    const api = create();
    const endpoint = await endpointOf(api, "ep-a-xn3000", "tcp_listener", { port: 4999 });
    const report = await api.testEndpoint({ endpoint });
    expect(report.ok).toBe(true);
    expect(names(report)).toEqual(["bind"]);
    expect(report.summary).toBe("This computer can listen on 10.0.4.5:4999.");
  });

  it("reports a port a running listener holds as in use, and a foreign address as not ours", async () => {
    const api = create();
    const running = await endpointOf(api, "ep-a-xn3000", "tcp_listener");
    const held = await api.testEndpoint({ endpoint: running });
    expect(held.ok).toBe(false);
    expect(held.steps).toHaveLength(1);
    expect(held.steps[0]?.detail).toBe(
      "port 4001 on 10.0.4.5 is already in use (by this engine or another program)",
    );
    const wildcard = { ...running, bindAddress: "0.0.0.0", id: "new" };
    expect((await api.testEndpoint({ endpoint: wildcard })).ok).toBe(false);
    const foreign = { ...running, bindAddress: "192.168.77.9", port: 4999 };
    expect((await api.testEndpoint({ endpoint: foreign })).steps[0]?.detail).toBe(
      "192.168.77.9 is not an address of this computer",
    );
  });

  it("opens a free serial port with its framing and refuses missing and held ones", async () => {
    const api = create();
    const free = await endpointOf(api, "ep-a-d100", "serial");
    const ok = await api.testEndpoint({ endpoint: free });
    expect(ok.ok).toBe(true);
    expect(names(ok)).toEqual(["open"]);
    expect(ok.steps[0]?.detail).toBe(
      "COM12 opened at 9600 baud, 8N1, no flow control, and closed again; nothing was sent",
    );
    for (const port of ["COM99", "/dev/ttyMISSING"]) {
      const report = await api.testEndpoint({ endpoint: { ...free, port } });
      expect(report.steps[0]).toMatchObject({ name: "open", ok: false, detail: "no such port" });
    }
    const held = await endpointOf(api, "ep-a-au5800", "serial");
    const inUse = await api.testEndpoint({ endpoint: { ...held, port: "com4", id: "copy" } });
    expect(inUse.steps).toHaveLength(1);
    expect(inUse.steps[0]).toMatchObject({ name: "open", ok: false, millis: 0 });
    expect(inUse.steps[0]?.detail).toMatch(/^in use by this engine \(channel .+\)$/);
  });

  it("walks a folder: find, count the matching files, prove it writable", async () => {
    const api = create();
    const endpoint = await endpointOf(api, "ep-qc-in", "folder");
    const report = await api.testEndpoint({ endpoint });
    expect(report.ok).toBe(true);
    expect(names(report)).toEqual(["find folder", "read directory", "write test"]);
    expect(report.steps[1]?.detail).toMatch(/^readable; \d+ files? match \*\.csv$/);
    const readOnly = { ...endpoint, path: "D:\\LabData\\readonly\\In" };
    const failed = await api.testEndpoint({ endpoint: readOnly });
    expect(failedStep(failed)).toBe("write test");
    expect(names(failed)).toEqual(["find folder", "read directory", "write test"]);
    const gone = await api.testEndpoint({ endpoint: { ...endpoint, path: "D:\\missing" } });
    expect(names(gone)).toEqual(["find folder"]);
  });

  it("walks SFTP through every step and stops at the one that fails", async () => {
    const api = create();
    const endpoint = await endpointOf(api, "ep-reflab-sftp", "sftp");
    const ok = await api.testEndpoint({ endpoint });
    expect(ok.ok).toBe(true);
    expect(names(ok)).toEqual([
      "credentials",
      "resolve",
      "connect",
      "authenticate",
      "open sftp",
      "list",
      "disconnect",
    ]);
    expect(ok.steps.every((step) => step.millis > 0)).toBe(true);

    const unstored = await api.testEndpoint({
      endpoint: { ...endpoint, secretName: "never-stored" },
    });
    expect(names(unstored)).toEqual(["credentials"]);
    expect(unstored.steps[0]?.detail).toBe('no secret named "never-stored" is stored');

    const mismatch = await api.testEndpoint({
      endpoint: { ...endpoint, hostKeyFingerprint: `SHA256:BAD${"a".repeat(40)}` },
    });
    expect(names(mismatch)).toEqual(["credentials", "resolve", "connect"]);
    expect(mismatch.summary).toMatch(/^The connect step failed: host key does not match/);
  });

  it("rejects a definition the endpoint rules refuse, at endpoints[0], before any step", async () => {
    const api = create();
    const endpoint = await endpointOf(api, "ep-lis-out", "tcp_client", { host: "  ", port: 0 });
    const error = await rejection(api.testEndpoint({ endpoint }));
    expect(error.code).toBe("validation");
    expect(error.fieldErrors.map((each) => each.field)).toEqual([
      "endpoints[0].host",
      "endpoints[0].port",
    ]);
    const empty = await rejection(
      api.testEndpoint({
        endpoint: await endpointOf(api, "ep-reflab-sftp", "sftp", { secretName: "" }),
      }),
    );
    expect(empty.fieldErrors.map((each) => each.field)).toContain("endpoints[0].secretName");
  });

  it("records each test as endpoint_test with what was tested, never a secret", async () => {
    const api = create();
    const all = { filter: { action: "endpoint_test" as const }, beforeSeq: null, limit: 50 };
    const before = (await api.getAuditEvents(all)).events.length;
    await api.testEndpoint({ endpoint: await endpointOf(api, "ep-lis-out", "tcp_client") });
    await api.testEndpoint({
      endpoint: await endpointOf(api, "ep-lis-out", "tcp_client", { host: "unreachable.example" }),
    });
    await rejection(
      api.testEndpoint({
        endpoint: await endpointOf(api, "ep-lis-out", "tcp_client", { port: 0 }),
      }),
    );
    const added = (await api.getAuditEvents(all)).events.slice(0, 7 - before).reverse();
    expect(added.map((event) => [event.outcome, event.details.ok])).toEqual([
      ["requested", undefined],
      ["succeeded", true],
      ["requested", undefined],
      ["succeeded", false],
      ["requested", undefined],
      ["rejected", undefined],
    ]);
    expect(added[0]?.details).toEqual({ kind: "tcp_client", target: "lis.lab.local:2575" });
    expect(added[0]).toMatchObject({ origin: "console", actor: expect.any(String) });
  });

  it("is refused while the audit log cannot be written", async () => {
    const api = create();
    api.setAuditAvailable(false);
    const error = await rejection(
      api.testEndpoint({ endpoint: await endpointOf(api, "ep-lis-out", "tcp_client") }),
    );
    expect(error.code).toBe("unavailable");
  });
});
