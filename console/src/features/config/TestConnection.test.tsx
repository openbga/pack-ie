import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { EndpointTestRequest, ProbeReport } from "@/api/types";
import { PASSED_PROBE as passed } from "@/lib/test/probeReport";
import { openEndpoint, openNewEndpoint, pick } from "./testActions";
import { engineFailure, stubApi } from "./testFixtures";

const failed: ProbeReport = {
  ok: false,
  summary: "The connect step failed: connection timed out.",
  steps: [
    { name: "resolve", ok: true, detail: "resolved to 10.0.4.21", millis: 12 },
    { name: "connect", ok: false, detail: "connection timed out", millis: 5000 },
  ],
};

function apiWith(testEndpoint: (request: EndpointTestRequest) => Promise<ProbeReport>) {
  const spy = vi.fn(testEndpoint);
  return Object.assign(stubApi({ testEndpoint: spy }), { testEndpoint: spy });
}

const testButton = () => screen.getByRole("button", { name: "Test connection" });

describe("Test connection in the endpoint editor", () => {
  it("offers the test only once a kind is chosen, with what it does and does not do", async () => {
    const { user } = await openNewEndpoint(apiWith(async () => passed));
    expect(screen.queryByRole("button", { name: "Test connection" })).toBeNull();
    await pick(user, document.body, "Kind", "TCP client");
    expect(screen.getByRole("group", { name: "Test connection" })).toBeTruthy();
    expect(
      screen.getByText(
        /No message or instrument data is sent, the configuration is not changed and no test file is left behind/,
      ),
    ).toBeTruthy();
  });

  it("tests the unsaved values of the form, shows the steps and leaves the form clean", async () => {
    const api = apiWith(async () => passed);
    const { user } = await openEndpoint("lis-out", "LIS host", api);
    await user.click(testButton());
    expect(await screen.findByText("Test passed")).toBeTruthy();
    expect(screen.getByText(passed.summary)).toBeTruthy();
    const [{ endpoint, timeoutSeconds }] = api.testEndpoint.mock.calls[0] ?? [{}];
    expect(endpoint).toMatchObject({
      kind: "tcp_client",
      id: "lis-out",
      host: "lis.example.test",
      port: 3001,
    });
    expect(timeoutSeconds).toBeUndefined();
    expect(
      within(screen.getByRole("table", { name: "Test steps" })).getAllByRole("row"),
    ).toHaveLength(3);
    expect(api.validateConfig).not.toHaveBeenCalled();
    expect(api.applyConfig).not.toHaveBeenCalled();
    // Nothing was changed, so leaving asks nothing.
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(await screen.findByText("Navigated to /endpoints/lis-out")).toBeTruthy();
  });

  it("sends what was typed, not what is saved, and marks the result stale on the next edit", async () => {
    const api = apiWith(async () => failed);
    const { user } = await openEndpoint("lis-out", "LIS host", api);
    const host = screen.getByRole("textbox", { name: /^Host/ });
    await user.clear(host);
    await user.type(host, "unreachable.example");
    await user.click(testButton());
    expect(await screen.findByText("Test failed")).toBeTruthy();
    expect(api.testEndpoint.mock.calls[0]?.[0].endpoint).toMatchObject({
      host: "unreachable.example",
    });
    expect(screen.getByText(/Nothing answered in time/)).toBeTruthy();
    expect(screen.queryByText(/These settings changed since the test/)).toBeNull();
    await user.type(host, "x");
    expect(screen.getByText(/These settings changed since the test/)).toBeTruthy();
    expect(screen.getByRole("table", { name: "Test steps" })).toBeTruthy();
    // Running again replaces it.
    await user.click(testButton());
    await waitFor(() => expect(api.testEndpoint).toHaveBeenCalledTimes(2));
    await waitFor(() =>
      expect(screen.queryByText(/These settings changed since the test/)).toBeNull(),
    );
    expect(api.testEndpoint.mock.calls[1]?.[0].endpoint).toMatchObject({
      host: "unreachable.examplex",
    });
  });

  it("shows the running state while the engine works", async () => {
    let finish: (report: ProbeReport) => void = () => undefined;
    const api = apiWith(() => new Promise<ProbeReport>((resolve) => (finish = resolve)));
    const { user } = await openEndpoint("lis-out", "LIS host", api);
    await user.click(testButton());
    expect(await screen.findByText("Testing…")).toBeTruthy();
    finish(passed);
    expect(await screen.findByText("Test passed")).toBeTruthy();
    expect(screen.queryByText("Testing…")).toBeNull();
  });

  it("asks for the required settings instead of calling the engine, and marks them", async () => {
    const api = apiWith(async () => passed);
    const { user } = await openNewEndpoint(api);
    await pick(user, document.body, "Kind", "TCP client");
    await user.click(testButton());
    expect(await screen.findByText("Fill in the required settings first")).toBeTruthy();
    expect(screen.getByText("Enter host.")).toBeTruthy();
    expect(api.testEndpoint).not.toHaveBeenCalled();
  });

  it("maps the engine's rejection onto the endpoint's own fields and says no test ran", async () => {
    const api = apiWith(async () => {
      throw engineFailure(
        "validation",
        "The endpoint's settings were rejected before any test ran.",
        [{ field: "endpoints[0].port", message: "Port must be a whole number from 1 to 65535." }],
      );
    });
    const { user } = await openEndpoint("lis-out", "LIS host", api);
    await user.click(testButton());
    expect(await screen.findByText("Settings rejected: no test ran")).toBeTruthy();
    const port = screen.getByRole("textbox", { name: /^Port/ });
    await waitFor(() => expect(port.getAttribute("aria-invalid")).toBe("true"));
    expect(screen.getByText("Port must be a whole number from 1 to 65535.")).toBeTruthy();
    expect(screen.queryByRole("table", { name: "Test steps" })).toBeNull();
  });

  it("shows an engine that is not running with the support reference and tries again", async () => {
    let calls = 0;
    const api = apiWith(async () => {
      calls += 1;
      if (calls === 1) throw engineFailure("engine_unavailable", "The engine is not running.");
      return passed;
    });
    const { user } = await openEndpoint("lis-out", "LIS host", api);
    await user.click(testButton());
    expect(await screen.findByText("The test could not run")).toBeTruthy();
    expect(screen.getByText("test-ref")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("Test passed")).toBeTruthy();
  });
});
