import { screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ConfigDocument, EndpointConfig, EndpointTestRequest, ProbeReport } from "@/api/types";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderRoutes } from "@/lib/test/router";
import {
  endpoint,
  endpointDetail,
  engineError,
  snapshot,
  stubApi,
} from "../shared-operate/testFixtures";
import { EndpointPage } from "./EndpointPage";

const routes = [{ path: "/endpoints/:id", element: <EndpointPage /> }];

const listener: EndpointConfig = {
  kind: "tcp_listener",
  id: "ep-lis",
  label: "LIS listener",
  enabled: true,
  bindAddress: "0.0.0.0",
  port: 3001,
  allowlist: ["10.0.4.12"],
  allowAnyPeer: false,
  idleTimeoutSeconds: 0,
  keepalive: true,
};

const document: ConfigDocument = {
  version: 3,
  endpoints: [listener],
  channels: [],
  alertThresholds: {
    endpointDownSeconds: 120,
    queueDepthWarning: 500,
    bufferPressurePercent: 85,
    diskFreeMinMib: 2048,
  },
};

const inUse: ProbeReport = {
  ok: false,
  summary:
    "The bind step failed: port 3001 on 0.0.0.0 is already in use (by this engine or another program).",
  steps: [
    {
      name: "bind",
      ok: false,
      detail: "port 3001 on 0.0.0.0 is already in use (by this engine or another program)",
      millis: 2,
    },
  ],
};

const bound: ProbeReport = {
  ok: true,
  summary: "This computer can listen on 0.0.0.0:3001.",
  steps: [
    { name: "bind", ok: true, detail: "0.0.0.0:3001 can be bound, then released", millis: 3 },
  ],
};

function api(
  held: string | null,
  testEndpoint: (request: EndpointTestRequest) => Promise<ProbeReport>,
  getConfig: () => Promise<ConfigDocument> = async () => document,
) {
  const spy = vi.fn(testEndpoint);
  const shown = endpoint({
    id: "ep-lis",
    label: "LIS listener",
    kind: "tcp_listener",
    state: held === null ? "disconnected" : "listening",
    heldBy: held,
  });
  return Object.assign(
    stubApi({
      getEndpoint: async () => endpointDetail(shown, []),
      getSnapshot: async () => snapshot({ endpoints: [shown] }),
      getConfig,
      testEndpoint: spy,
    }),
    { testEndpoint: spy },
  );
}

const open = async (engine: ReturnType<typeof api>) => {
  const view = renderRoutes(routes, "/endpoints/ep-lis", { api: engine });
  await screen.findByRole("heading", { level: 1, name: "LIS listener" });
  return view;
};

describe("Test connection on the endpoint page", () => {
  it("tests the saved settings from the configuration and shows the steps", async () => {
    const engine = api(null, async () => bound);
    const { user, container } = await open(engine);
    expect(screen.queryByText(/is reported as in use/)).toBeNull();
    await user.click(await screen.findByRole("button", { name: "Test connection" }));
    expect(await screen.findByText("Test passed")).toBeTruthy();
    expect(screen.getByText(bound.summary)).toBeTruthy();
    expect(engine.testEndpoint).toHaveBeenCalledWith({ endpoint: listener });
    expect(screen.getByRole("table", { name: "Test steps" })).toBeTruthy();
    await expectNoA11yViolations(container);
  });

  it("explains that a port a channel holds reports in use while the endpoint works", async () => {
    const engine = api("ch-hl7", async () => inUse);
    const { user, container } = await open(engine);
    expect(
      await screen.findByText(/the endpoint is working if a channel is running it/),
    ).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Test connection" }));
    expect(await screen.findByText("Test failed")).toBeTruthy();
    expect(
      screen.getByText(/A running listener, or another program, holds this port/),
    ).toBeTruthy();
    await expectNoA11yViolations(container);
  });

  it("lists rejected settings, since there is no form to mark them on", async () => {
    const engine = api(null, async () => {
      throw {
        ...engineError("validation"),
        fieldErrors: [
          { field: "endpoints[0].port", message: "Port must be a whole number from 1 to 65535." },
        ],
      };
    });
    const { user } = await open(engine);
    await user.click(await screen.findByRole("button", { name: "Test connection" }));
    expect(await screen.findByText("Settings rejected: no test ran")).toBeTruthy();
    expect(screen.getByText("Port must be a whole number from 1 to 65535.")).toBeTruthy();
  });

  it("shows an engine that cannot be reached with Try again", async () => {
    let calls = 0;
    const engine = api(null, async () => {
      calls += 1;
      if (calls === 1) throw engineError("engine_unavailable");
      return bound;
    });
    const { user } = await open(engine);
    await user.click(await screen.findByRole("button", { name: "Test connection" }));
    expect(await screen.findByText("The test could not run")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("Test passed")).toBeTruthy();
  });

  it("says when the saved settings cannot be loaded, and offers to try again", async () => {
    let calls = 0;
    const engine = api(
      null,
      async () => bound,
      async () => {
        calls += 1;
        if (calls === 1) throw engineError("engine_unavailable");
        return document;
      },
    );
    const { user } = await open(engine);
    expect(await screen.findByText("The saved settings could not be loaded")).toBeTruthy();
    expect(engine.testEndpoint).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Test connection" })).toBeTruthy(),
    );
  });
});
