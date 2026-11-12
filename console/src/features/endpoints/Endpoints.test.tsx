import { screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { columnTexts, openRow } from "@/lib/test/grid";
import { chooseWithKeyboard } from "@/lib/test/keyboard";
import { mockGridLayout } from "@/lib/test/layout";
import { renderRoutes } from "@/lib/test/router";
import {
  ago,
  channel,
  endpoint,
  endpointDetail,
  engineError,
  messageChannel,
  snapshot,
  stubApi,
} from "../shared-operate/testFixtures";
import { EndpointPage } from "./EndpointPage";
import { EndpointsPage } from "./EndpointsPage";

const listRoutes = [{ path: "/endpoints", element: <EndpointsPage /> }];
const pageRoutes = [{ path: "/endpoints/:id", element: <EndpointPage /> }];

const failing = endpoint({
  id: "ep-bad",
  label: "Archive SFTP",
  kind: "sftp",
  state: "error",
  summary: "sftp://archive.example.test:22",
  channelIds: ["ch-alpha", "ch-hl7"],
  stateSince: ago(300),
  lastError: {
    at: ago(20),
    summary: "Authentication failed",
    nextAction: "Check the SFTP secret in Settings.",
  },
});
const data = () =>
  snapshot({
    channels: [channel(), channel({ id: "ch-hl7", label: "Results to LIS" })],
    endpoints: [
      endpoint(),
      failing,
      endpoint({ id: "ep-lis", label: "LIS listener", kind: "tcp_listener", state: "listening" }),
    ],
  });

const rowLabels = () => columnTexts(0, "Endpoints");

beforeEach(() => mockGridLayout());

describe("EndpointsPage", () => {
  it("lists endpoints with the failing one first, its state, age and channels using it", async () => {
    const { container } = renderRoutes(listRoutes, "/endpoints", {
      api: stubApi({ getSnapshot: async () => data() }),
    });
    expect(await screen.findByRole("grid", { name: "Endpoints" })).toBeTruthy();
    expect(rowLabels()).toEqual(["Archive SFTP", "Analyzer COM3", "LIS listener"]);
    const grid = screen.getByRole("grid", { name: "Endpoints" });
    expect(within(grid).getByText("Error")).toBeTruthy();
    expect(within(grid).getByText("Alpha bridge and 1 more")).toBeTruthy();
    expect(within(grid).getByText("Authentication failed")).toBeTruthy();
    expect(screen.getByText("3 endpoints")).toBeTruthy();
    expect(screen.getByRole("link", { name: "Add endpoint" }).getAttribute("href")).toBe(
      "/endpoints/new",
    );
    await expectNoA11yViolations(container);
  });

  it("shows kind, address, bytes in and out, last activity, failure streak and last error", async () => {
    const row = endpoint({
      id: "ep-x",
      label: "Busy port",
      summary: "COM7 · 19200 8N1",
      consecutiveFailureCount: 4,
      counters: { bytesIn: 3 * 1024, bytesOut: 1024, messagesIn: 0, messagesOut: 0, retries: 0 },
      lastError: { at: ago(30), summary: "Read failed", nextAction: "Check the cable." },
    });
    renderRoutes(listRoutes, "/endpoints", {
      api: stubApi({ getSnapshot: async () => snapshot({ endpoints: [row] }) }),
    });
    const grid = await screen.findByRole("grid", { name: "Endpoints" });
    const cells = within(within(grid).getAllByRole("row")[1] as HTMLElement)
      .getAllByRole("gridcell")
      .map((cell) => cell.textContent);
    expect(cells).toContain("Serial");
    expect(cells).toContain("COM7 · 19200 8N1");
    expect(cells).toContain("3.0 KB");
    expect(cells).toContain("1.0 KB");
    expect(cells).toContain("4");
    expect(cells).toContain("Read failed");
    expect(cells.some((cell) => cell?.startsWith("Connected"))).toBe(true);
    expect(cells.some((cell) => cell?.endsWith("1 min ago") || cell?.endsWith("1 h ago"))).toBe(
      true,
    );
  });

  it("filters by kind and by state, kept in the address, and clears them", async () => {
    const { user, router } = renderRoutes(listRoutes, "/endpoints", {
      api: stubApi({ getSnapshot: async () => data() }),
    });
    await screen.findByRole("grid", { name: "Endpoints" });

    await chooseWithKeyboard(user, /Kind/, "Serial");
    await waitFor(() => expect(rowLabels()).toEqual(["Analyzer COM3"]));
    expect(screen.getByText("1 of 3 endpoints")).toBeTruthy();
    expect(router.state.location.search).toBe("?kind=serial");

    await chooseWithKeyboard(user, /State/, "Error");
    expect(await screen.findByText("No endpoints match the filters")).toBeTruthy();

    await user.click(screen.getAllByRole("button", { name: "Clear filters" })[0] as HTMLElement);
    await waitFor(() => expect(rowLabels()).toHaveLength(3));
    expect(router.state.location.search).toBe("");
  });

  it("opens an endpoint with one click on its row", async () => {
    const { user } = renderRoutes(listRoutes, "/endpoints", {
      api: stubApi({ getSnapshot: async () => data() }),
    });
    await openRow(user, "Analyzer COM3");
    expect(await screen.findByText("Navigated to /endpoints/ep-com3")).toBeTruthy();
  });

  it("offers the first endpoint when there are none", async () => {
    renderRoutes(listRoutes, "/endpoints", {
      api: stubApi({ getSnapshot: async () => snapshot({ endpoints: [] }) }),
    });
    expect(await screen.findByText("No endpoints yet")).toBeTruthy();
  });

  it("shows an error with retry when the engine cannot answer", async () => {
    renderRoutes(listRoutes, "/endpoints", {
      api: stubApi({
        getSnapshot: async () => {
          throw engineError("engine_unavailable");
        },
      }),
    });
    expect(await screen.findByText("The endpoints could not be loaded")).toBeTruthy();
  });
});

describe("EndpointPage", () => {
  it("shows state, last error with next step, counters and the channels that use it", async () => {
    const { container } = renderRoutes(pageRoutes, "/endpoints/ep-bad", {
      api: stubApi({
        getEndpoint: async () =>
          endpointDetail(failing, [
            { id: "ch-alpha", label: "Alpha bridge", role: "Side A" },
            { id: "ch-hl7", label: "Results to LIS", role: "Destination" },
          ]),
        getSnapshot: async () => data(),
      }),
    });
    expect(await screen.findByRole("heading", { level: 1, name: "Archive SFTP" })).toBeTruthy();
    const note = screen.getByText("Authentication failed").closest("[role=alert]");
    expect(note?.textContent).toContain("Check the SFTP secret in Settings.");
    expect(screen.getByRole("table", { name: "Endpoint counters" })).toBeTruthy();

    const using = screen.getByRole("table", { name: "Channels using this endpoint" });
    expect(within(using).getByRole("link", { name: "Results to LIS" }).getAttribute("href")).toBe(
      "/channels/ch-hl7",
    );
    expect(screen.getByRole("link", { name: "Edit" }).getAttribute("href")).toBe(
      "/endpoints/ep-bad/edit",
    );
    expect(screen.getByRole("link", { name: "Endpoints" }).getAttribute("href")).toBe("/endpoints");
    await expectNoA11yViolations(container);
  });

  it("says when no channel uses the endpoint", async () => {
    renderRoutes(pageRoutes, "/endpoints/ep-com3", {
      api: stubApi({
        getEndpoint: async () => endpointDetail(endpoint(), []),
        getSnapshot: async () => data(),
      }),
    });
    expect(await screen.findByText("No channel uses this endpoint")).toBeTruthy();
  });

  it("says the endpoint was not found, with a way back", async () => {
    renderRoutes(pageRoutes, "/endpoints/gone", {
      api: stubApi({
        getEndpoint: async () => {
          throw engineError("not_found");
        },
        getSnapshot: async () => data(),
      }),
    });
    expect(
      await screen.findByRole("heading", { level: 1, name: "Endpoint not found" }),
    ).toBeTruthy();
    expect(screen.getByRole("link", { name: "Back to endpoints" }).getAttribute("href")).toBe(
      "/endpoints",
    );
  });
});

describe("EndpointPage, operating", () => {
  const held = endpoint({
    id: "ep-lis",
    label: "LIS listener",
    kind: "tcp_listener",
    state: "retrying",
    summary: "0.0.0.0:3001",
    heldBy: "ch-hl7",
    activePeer: "10.0.4.12:51044",
    consecutiveFailureCount: 3,
    nextRetryAt: new Date(Date.now() + 8000).toISOString(),
    channelIds: ["ch-alpha", "ch-hl7"],
    counters: { bytesIn: 2048, bytesOut: 1024, messagesIn: 12, messagesOut: 11, retries: 0 },
    lastError: {
      at: ago(10),
      summary:
        'Channel ch-hl7 source endpoint "LIS listener" accept failed: address in use. Free the port.',
      nextAction: "Stop the program using port 3001.",
    },
  });
  const using = [
    { id: "ch-alpha", label: "Alpha bridge", role: "Side B" },
    { id: "ch-hl7", label: "Results to LIS", role: "source" },
  ];
  const live = () =>
    snapshot({
      channels: [channel(), messageChannel()],
      endpoints: [held],
      openAlerts: [
        {
          id: "al-1",
          severity: "warning",
          kind: "endpoint_down",
          title: "Endpoint down",
          detail: "",
          channelId: null,
          endpointId: "ep-lis",
          firstSeenAt: ago(60),
          lastSeenAt: ago(5),
          occurrences: 2,
          state: "open",
          clearedAt: null,
          nextAction: "Check the port.",
        },
      ],
    });
  const api = (extra: Record<string, unknown> = {}) =>
    stubApi({
      getEndpoint: async () => endpointDetail(held, using),
      getSnapshot: async () => live(),
      ...extra,
    });

  it("shows who holds it, its peer, failure streak, next retry, counters, alerts and the structured error", async () => {
    const { container } = renderRoutes(pageRoutes, "/endpoints/ep-lis", { api: api() });
    await screen.findByRole("heading", { level: 1, name: "LIS listener" });
    const facts = screen.getByText("Endpoint id").closest("dl") as HTMLElement;
    expect(facts.textContent).toContain("Points to0.0.0.0:3001");
    expect(facts.textContent).toContain("Active peer10.0.4.12:51044");
    expect(facts.textContent).toContain("Failure streak3");
    expect(facts.textContent).toMatch(/Next retryin [1-8] s/);
    expect(within(facts).getByRole("link", { name: "Results to LIS" }).getAttribute("href")).toBe(
      "/channels/ch-hl7",
    );

    const counters = screen.getByRole("table", { name: "Endpoint counters" });
    await waitFor(() => expect(counters.textContent).toContain("Messages"));
    expect(counters.textContent).toContain("2.0 KB");

    const error = screen.getByText("Last error").closest("[role=alert]") as HTMLElement;
    expect(error.querySelector("dl")?.textContent).toContain("Operationaccept");
    expect(error.querySelector("dl")?.textContent).toContain("Causeaddress in use");
    expect(error.querySelector("dl")?.textContent).toContain("Failure streak3");

    expect(
      screen.getByRole("list", { name: "Active alerts of this endpoint" }).textContent,
    ).toContain("Check the port.");
    const usingTable = screen.getByRole("table", { name: "Channels using this endpoint" });
    expect(within(usingTable).getAllByRole("row")[2]?.textContent).toContain("Yes");
    await expectNoA11yViolations(container);
  });

  it("leaves out messages where they do not apply, and handles no holder and no peer", async () => {
    const quiet = endpoint({ heldBy: null, activePeer: null });
    renderRoutes(pageRoutes, "/endpoints/ep-com3", {
      api: api({
        getEndpoint: async () => endpointDetail(quiet, [using[0] as (typeof using)[number]]),
        getSnapshot: async () => snapshot({ channels: [channel()], endpoints: [quiet] }),
      }),
    });
    await screen.findByRole("heading", { level: 1, name: "Analyzer COM3" });
    const facts = screen.getByText("Endpoint id").closest("dl") as HTMLElement;
    expect(facts.textContent).toContain("Held byNo channel");
    expect(facts.textContent).toContain("Active peerNone");
    expect(facts.textContent).toContain("Next retryNone scheduled");
    expect(screen.getByRole("table", { name: "Endpoint counters" }).textContent).not.toContain(
      "Messages",
    );
  });

  it("restarts the channels that use it after a confirmation that lists them", async () => {
    const restartChannel = vi.fn(async () => undefined);
    const paused = channel({
      id: "ch-paused",
      label: "Paused bridge",
      state: "stopped",
      stopReason: "paused",
    });
    const { user } = renderRoutes(pageRoutes, "/endpoints/ep-lis", {
      api: api({
        restartChannel,
        getEndpoint: async () =>
          endpointDetail(held, [
            ...using,
            { id: "ch-paused", label: "Paused bridge", role: "Side A" },
          ]),
        getSnapshot: async () =>
          snapshot({ channels: [channel(), messageChannel(), paused], endpoints: [held] }),
      }),
    });
    await user.click(
      await screen.findByRole("button", { name: "Restart the channels that use it" }),
    );
    const dialog = await screen.findByRole("alertdialog", {
      name: "Restart the channels that use LIS listener?",
    });
    expect(dialog.textContent).toContain("counters of each restarted instance reset");
    const restarting = within(dialog).getByRole("list", { name: "Channels to restart" });
    expect(restarting.textContent).toBe("Alpha bridgeResults to LIS");
    expect(within(dialog).getByRole("list", { name: "Channels left alone" }).textContent).toBe(
      "Paused bridge",
    );
    expect(restartChannel).not.toHaveBeenCalled();

    await user.click(within(dialog).getByRole("button", { name: "Restart 2 channels" }));
    await waitFor(() => expect(restartChannel).toHaveBeenCalledTimes(2));
    expect(restartChannel).toHaveBeenCalledWith("ch-alpha");
    expect(restartChannel).toHaveBeenCalledWith("ch-hl7");
    expect(await screen.findByText("Restarting 2 channels.")).toBeTruthy();
  });

  it("reports a refusal per channel in the engine's words", async () => {
    const restartChannel = vi.fn(async (id: string) => {
      if (id === "ch-hl7") throw engineError("conflict", "Channel is paused: use Resume");
    });
    const { user } = renderRoutes(pageRoutes, "/endpoints/ep-lis", {
      api: api({ restartChannel }),
    });
    await user.click(
      await screen.findByRole("button", { name: "Restart the channels that use it" }),
    );
    await user.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Restart 2 channels",
      }),
    );
    expect(await screen.findByText("1 of 2 could not be restarted")).toBeTruthy();
    expect(screen.getByText("Results to LIS: Channel is paused: use Resume")).toBeTruthy();
  });

  it("offers no restart shortcut when no channel can be restarted", async () => {
    const paused = channel({ state: "stopped", stopReason: "paused" });
    renderRoutes(pageRoutes, "/endpoints/ep-com3", {
      api: api({
        getEndpoint: async () => endpointDetail(endpoint(), [using[0] as (typeof using)[number]]),
        getSnapshot: async () => snapshot({ channels: [paused], endpoints: [endpoint()] }),
      }),
    });
    await screen.findByRole("heading", { level: 1, name: "Analyzer COM3" });
    expect(screen.queryByRole("button", { name: "Restart the channels that use it" })).toBeNull();
  });
});
