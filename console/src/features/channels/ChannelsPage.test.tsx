import { screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { columnTexts, openRow, openRowActions } from "@/lib/test/grid";
import { mockGridLayout } from "@/lib/test/layout";
import { renderRoutes } from "@/lib/test/router";
import {
  channel,
  engineError,
  failingChannel,
  messageChannel,
  snapshot,
  stubApi,
} from "../shared-operate/testFixtures";
import { ChannelsPage } from "./ChannelsPage";

const routes = [{ path: "/channels", element: <ChannelsPage /> }];
const data = () =>
  snapshot({
    channels: [
      channel({ id: "ch-alpha", label: "Alpha bridge" }),
      failingChannel(),
      messageChannel({ queue: { depth: 9, needsAttention: 3, oldestQueuedAt: null } }),
    ],
  });

const rowLabels = () => columnTexts(0, "Channels");

beforeEach(() => mockGridLayout());

describe("ChannelsPage", () => {
  it("lists channels failing first, with a count that names what it counts", async () => {
    const { container } = renderRoutes(routes, "/channels", {
      api: stubApi({ getSnapshot: async () => data() }),
    });
    expect(await screen.findByRole("grid", { name: "Channels" })).toBeTruthy();
    expect(rowLabels()).toEqual(["Zulu bridge", "Results to LIS", "Alpha bridge"]);
    expect(screen.getByText("3 channels")).toBeTruthy();
    const grid = screen.getByRole("grid", { name: "Channels" });
    expect(within(grid).getByText("Degraded")).toBeTruthy();
    expect(within(grid).getByText("3 need attention")).toBeTruthy();
    expect(screen.getByRole("link", { name: "Add channel" }).getAttribute("href")).toBe(
      "/channels/new",
    );
    await expectNoA11yViolations(container);
  });

  it("finds by channel or endpoint name, kept in the address", async () => {
    const { user, router } = renderRoutes(routes, "/channels", {
      api: stubApi({ getSnapshot: async () => data() }),
    });
    await screen.findByRole("grid", { name: "Channels" });

    await user.type(screen.getByRole("searchbox", { name: "Find channels" }), "chemistry{Enter}");
    await waitFor(() => expect(rowLabels()).toEqual(["Results to LIS"]));
    expect(screen.getByText("1 of 3 channels")).toBeTruthy();
    expect(router.state.location.search).toBe("?q=chemistry");
  });

  it("narrows to channels that need attention and explains an empty result", async () => {
    const { user, router } = renderRoutes(routes, "/channels", {
      api: stubApi({ getSnapshot: async () => data() }),
    });
    await screen.findByRole("grid", { name: "Channels" });

    await user.click(screen.getByRole("checkbox", { name: "Needs attention only" }));
    await waitFor(() => expect(rowLabels()).toEqual(["Zulu bridge", "Results to LIS"]));
    expect(screen.getByText("2 of 3 channels")).toBeTruthy();
    expect(router.state.location.search).toBe("?attention=1");

    await user.type(screen.getByRole("searchbox", { name: "Find channels" }), "nothing{Enter}");
    expect(await screen.findByText("No channels match 'nothing'")).toBeTruthy();
    const empty = screen.getByText("No channels match 'nothing'").parentElement as HTMLElement;
    await user.click(within(empty).getByRole("button", { name: "Clear search" }));
    await waitFor(() => expect(rowLabels()).toHaveLength(2));
  });

  it("opens a channel with one click on its row", async () => {
    const { user } = renderRoutes(routes, "/channels", {
      api: stubApi({ getSnapshot: async () => data() }),
    });
    await openRow(user, "Alpha bridge");
    expect(await screen.findByText("Navigated to /channels/ch-alpha")).toBeTruthy();
  });

  it("pauses a channel from its row menu only after confirming the consequence", async () => {
    const pauseChannel = vi.fn(async () => undefined);
    const { user } = renderRoutes(routes, "/channels", {
      api: stubApi({ getSnapshot: async () => data(), pauseChannel }),
    });
    await screen.findByRole("grid", { name: "Channels" });

    const menu = await openRowActions(user, "Zulu bridge");
    expect(within(menu).getByRole("menuitem", { name: "Restart" })).toBeTruthy();
    expect(within(menu).getByRole("menuitem", { name: "Edit" })).toBeTruthy();
    await user.click(within(menu).getByRole("menuitem", { name: "Pause" }));

    const dialog = await screen.findByRole("alertdialog", { name: "Pause Zulu bridge?" });
    expect(dialog.textContent).toContain("The session closes");
    expect(pauseChannel).not.toHaveBeenCalled();
    await user.click(within(dialog).getByRole("button", { name: "Pause channel" }));

    await waitFor(() => expect(pauseChannel).toHaveBeenCalledWith("ch-zulu"));
    expect(await screen.findByText("Channel paused. No retry is scheduled.")).toBeTruthy();
  });

  it("offers Resume, not Pause, on a paused channel, and only Edit on a disabled one", async () => {
    const paused = channel({
      id: "ch-p",
      label: "Paused one",
      state: "stopped",
      stopReason: "paused",
    });
    const disabled = channel({
      id: "ch-d",
      label: "Disabled one",
      state: "stopped",
      stopReason: "disabled",
    });
    const { user } = renderRoutes(routes, "/channels", {
      api: stubApi({ getSnapshot: async () => snapshot({ channels: [paused, disabled] }) }),
    });
    await screen.findByRole("grid", { name: "Channels" });
    let menu = await openRowActions(user, "Paused one");
    expect(
      within(menu)
        .getAllByRole("menuitem")
        .map((item) => item.textContent),
    ).toEqual(["Resume", "Edit"]);
    await user.keyboard("{Escape}");
    menu = await openRowActions(user, "Disabled one");
    expect(
      within(menu)
        .getAllByRole("menuitem")
        .map((item) => item.textContent),
    ).toEqual(["Edit"]);
  });

  it("offers the first channel when there are none, and shows engine errors with a retry", async () => {
    const { unmount } = renderRoutes(routes, "/channels", {
      api: stubApi({ getSnapshot: async () => snapshot({ channels: [] }) }),
    });
    expect(await screen.findByText("No channels yet")).toBeTruthy();
    expect(screen.getAllByRole("link", { name: "Add channel" })).toHaveLength(2);
    unmount();

    renderRoutes(routes, "/channels", {
      api: stubApi({
        getSnapshot: async () => {
          throw engineError("engine_unavailable", "The engine is not running.");
        },
      }),
    });
    expect(await screen.findByText("The channels could not be loaded")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Try again" })).toBeTruthy();
  });
});
