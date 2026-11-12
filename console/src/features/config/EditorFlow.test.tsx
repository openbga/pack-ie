import { act, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { MockEngineApi } from "@/api/mock";
import type { ApplyPlan, ChannelDetail, ValidationResult } from "@/api/types";
import { renderRoutes } from "@/lib/test/router";
import { ChannelEditPage } from "./ChannelEditPage";
import { EndpointEditPage } from "./EndpointEditPage";
import { removeSavedDestination, typeNote } from "./testActions";
import { NO_PLAN, sampleConfig, stubApi } from "./testFixtures";

const routes = [
  { path: "/channels/new", element: <ChannelEditPage /> },
  { path: "/channels/:id/edit", element: <ChannelEditPage /> },
  { path: "/endpoints/:id/edit", element: <EndpointEditPage /> },
];

/** The record's own label; a destination has one too. */
const labelField = () =>
  within(screen.getByRole("group", { name: "General" })).getByRole("textbox", { name: /^Label/ });

describe("editing one record after another", () => {
  it("shows the channel of the address after navigating between edit URLs", async () => {
    const api = stubApi();
    const { user, router } = renderRoutes(routes, "/channels/chem-bridge/edit", { api });
    await screen.findByRole("heading", { level: 1, name: "Chemistry bridge" });
    expect((labelField() as HTMLInputElement).value).toBe("Chemistry bridge");

    await act(() => router.navigate("/channels/results/edit"));
    await screen.findByRole("heading", { level: 1, name: "Results to LIS" });
    expect(screen.getByRole("form", { name: "Edit channel Results to LIS" })).toBeTruthy();
    expect((labelField() as HTMLInputElement).value).toBe("Results to LIS");
    expect(screen.getByRole("group", { name: "Source" })).toBeTruthy();
    expect(screen.queryByRole("combobox", { name: /Endpoint A/ })).toBeNull();

    // What the delete acts on is the record shown.
    await user.click(screen.getByRole("button", { name: "Delete channel…" }));
    const dialog = await screen.findByRole("alertdialog", {
      name: "Delete channel Results to LIS?",
    });
    await typeNote(user, "Retire the results channel");
    await user.click(within(dialog).getByRole("button", { name: "Delete channel" }));
    await screen.findByText("Navigated to /channels");
    const [document] = api.applyConfig.mock.calls[0] ?? [];
    expect(document?.channels.map((channel) => channel.id)).toEqual(["chem-bridge"]);
  });

  it("shows the endpoint of the address after navigating between edit URLs", async () => {
    const { router } = renderRoutes(routes, "/endpoints/lis-out/edit", { api: stubApi() });
    await screen.findByRole("heading", { level: 1, name: "LIS host" });
    await act(() => router.navigate("/endpoints/chem-serial/edit"));
    await screen.findByRole("heading", { level: 1, name: "Chemistry serial" });
    expect((labelField() as HTMLInputElement).value).toBe("Chemistry serial");
    expect(screen.getByRole("combobox", { name: /^Port/ })).toHaveProperty("value", "COM3");
    expect(screen.queryByRole("textbox", { name: /^Host/ })).toBeNull();
  });

  it("goes from a new channel to an existing one with an empty form behind it", async () => {
    const { router } = renderRoutes(routes, "/channels/new", { api: stubApi() });
    await screen.findByRole("heading", { level: 1, name: "New channel" });
    await act(() => router.navigate("/channels/chem-bridge/edit"));
    await screen.findByRole("heading", { level: 1, name: "Chemistry bridge" });
    expect((labelField() as HTMLInputElement).value).toBe("Chemistry bridge");
    expect(screen.queryByRole("combobox", { name: /^Kind/ })).toBeNull();
  });

  it("asks before leaving unsaved edits for another edit URL, and keeps them when told to stay", async () => {
    const { user, router } = renderRoutes(routes, "/channels/chem-bridge/edit", {
      api: stubApi(),
    });
    await screen.findByRole("heading", { level: 1, name: "Chemistry bridge" });
    await user.type(labelField(), " edited");

    void router.navigate("/channels/results/edit");
    const dialog = await screen.findByRole("alertdialog", { name: "Leave without saving?" });
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(router.state.location.pathname).toBe("/channels/chem-bridge/edit");
    expect((labelField() as HTMLInputElement).value).toBe("Chemistry bridge edited");

    void router.navigate("/channels/results/edit");
    await user.click(
      within(await screen.findByRole("alertdialog", { name: "Leave without saving?" })).getByRole(
        "button",
        { name: "Leave without saving" },
      ),
    );
    await screen.findByRole("heading", { level: 1, name: "Results to LIS" });
    expect((labelField() as HTMLInputElement).value).toBe("Results to LIS");
  });
});

describe("deleting and removing: what happens to deliveries", () => {
  it("says a deleted channel's unresolved deliveries are closed, with the count the snapshot gives", async () => {
    const api = new MockEngineApi({ latency: "none", messageCount: 500 });
    const snapshot = await api.getSnapshot();
    const channel = snapshot.channels.find((each) => each.id === "ch-msg-sta");
    if (channel?.queue == null) throw new Error("the mock has no such message channel");
    const { user } = renderRoutes(routes, "/channels/ch-msg-sta/edit", { api });
    await screen.findByRole("heading", { level: 1, name: channel.label });
    await user.click(screen.getByRole("button", { name: "Delete channel…" }));
    const dialog = await screen.findByRole("alertdialog");
    expect(within(dialog).getByText(/other channels keep running/)).toBeTruthy();
    expect(
      within(dialog).getByText(
        /Unresolved deliveries of this channel are closed when it is removed/,
      ),
    ).toBeTruthy();
    expect(
      within(dialog).getByText(
        new RegExp(`Right now ${channel.queue.needsAttention} need attention`),
      ),
    ).toBeTruthy();
  });

  it("says so without a count while the engine reports none", async () => {
    const { user } = renderRoutes(routes, "/channels/results/edit", { api: stubApi() });
    await screen.findByRole("heading", { level: 1, name: "Results to LIS" });
    await user.click(screen.getByRole("button", { name: "Delete channel…" }));
    const dialog = await screen.findByRole("alertdialog");
    expect(
      within(dialog).getByText(/Unresolved deliveries of this channel are closed/),
    ).toBeTruthy();
    expect(within(dialog).queryByText(/need attention/)).toBeNull();
  });

  function withDestinationCounts() {
    return stubApi({
      getChannel: async () =>
        ({
          snapshot: {},
          config: {},
          destinations: [
            {
              destinationId: "d1",
              unresolved: { rejected: 1, failed: 2, timed_out: 0, outcome_unknown: 1 },
              queued: 3,
            },
          ],
        }) as unknown as ChannelDetail,
    });
  }

  it("asks before removing a destination the channel has, says what happens to its deliveries and removes it only when told to", async () => {
    const { user } = renderRoutes(routes, "/channels/results/edit", {
      api: withDestinationCounts(),
    });
    await screen.findByRole("heading", { level: 1, name: "Results to LIS" });
    await user.click(screen.getByRole("button", { name: "Remove destination 1" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Remove destination 1 (LIS)?" });
    expect(await within(dialog).findByText(/Right now 4 need attention and 3 queued/)).toBeTruthy();
    expect(within(dialog).getByText(/closed when it is removed/)).toBeTruthy();
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.getByRole("group", { name: "Destination 1" })).toBeTruthy();

    await removeSavedDestination(user, 1);
    expect(screen.queryByRole("group", { name: "Destination 1" })).toBeNull();
  });

  it("removes a destination added in this form without asking", async () => {
    const { user } = renderRoutes(routes, "/channels/results/edit", { api: stubApi() });
    await screen.findByRole("heading", { level: 1, name: "Results to LIS" });
    await user.click(screen.getByRole("button", { name: "Add destination" }));
    expect(screen.getByRole("group", { name: "Destination 2" })).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Remove destination 2" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(screen.queryByRole("group", { name: "Destination 2" })).toBeNull();
  });

  it("repeats it in the confirmation that applies the removal", async () => {
    const config = sampleConfig();
    const results = config.channels.find((channel) => channel.id === "results");
    if (results?.kind !== "message") throw new Error("fixture");
    const first = results.destinations[0];
    if (first === undefined) throw new Error("fixture");
    results.destinations = [first, { ...first, id: "d2", label: "Backup LIS" }];
    const api = stubApi({ getConfig: async () => config });
    const { user } = renderRoutes(routes, "/channels/results/edit", { api });
    await screen.findByRole("heading", { level: 1, name: "Results to LIS" });
    await removeSavedDestination(user, 2);
    await user.click(screen.getByRole("button", { name: "Save and apply" }));
    const dialog = await screen.findByRole("alertdialog", {
      name: "Apply configuration version 8?",
    });
    expect(
      within(dialog).getByText(
        /Unresolved deliveries of a removed channel or destination are closed/,
      ),
    ).toBeTruthy();
    expect(
      within(dialog).getByText("Destination Backup LIS of Results to LIS is removed."),
    ).toBeTruthy();
  });
});

const ERROR: ValidationResult = {
  ok: false,
  plan: NO_PLAN,
  issues: [
    {
      severity: "error",
      path: "channels[0].retry.maxMs",
      message: "The longest retry delay must not be shorter than the first.",
      code: "retry_invalid",
    },
  ],
};

const PLAN: ApplyPlan = {
  channels: [
    { id: "chem-bridge", label: "Chemistry bridge", change: "changed" },
    { id: "results", label: "Results to LIS", change: "updated" },
    { id: "billing", label: "Billing", change: "added" },
    { id: "paused", label: "Paused feed", change: "stopped" },
    { id: "old", label: "Old feed", change: "removed" },
    { id: "a", label: "A", change: "unchanged" },
    { id: "b", label: "B", change: "unchanged" },
  ],
  endpoints: [{ id: "lis-out", label: "LIS host", change: "changed" }],
  alertsChanged: true,
};

describe("validating and applying", () => {
  it("shows each problem's code, and keeps Save and apply off until the document validates", async () => {
    let result: ValidationResult = ERROR;
    const api = stubApi({ validateConfig: async () => result });
    const { user } = renderRoutes(routes, "/channels/chem-bridge/edit", { api });
    await screen.findByRole("heading", { level: 1, name: "Chemistry bridge" });
    const apply = () => screen.getByRole("button", { name: "Save and apply" });
    expect(apply()).toHaveProperty("disabled", false);

    await user.click(screen.getByRole("button", { name: "Validate" }));
    const results = await screen.findByRole("region", { name: "Validation results" });
    expect(within(results).getByText("channels[0].retry.maxMs · retry_invalid")).toBeTruthy();
    expect(within(results).getByText(/Save and apply stays off until you fix them/)).toBeTruthy();
    expect(apply()).toHaveProperty("disabled", true);

    result = { ok: true, issues: [], plan: NO_PLAN };
    await user.click(screen.getByRole("button", { name: "Validate" }));
    await screen.findByText("No problems found");
    expect(apply()).toHaveProperty("disabled", false);
  });

  it("validates first on Save and apply, and opens the confirmation only for a valid document", async () => {
    let result: ValidationResult = ERROR;
    const api = stubApi({ validateConfig: vi.fn(async () => result) });
    const { user } = renderRoutes(routes, "/channels/chem-bridge/edit", { api });
    await screen.findByRole("heading", { level: 1, name: "Chemistry bridge" });

    await user.click(screen.getByRole("button", { name: "Save and apply" }));
    expect(await screen.findByRole("region", { name: "Validation results" })).toBeTruthy();
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(api.applyConfig).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Save and apply" })).toHaveProperty("disabled", true);

    result = { ok: true, issues: [], plan: NO_PLAN };
    await user.click(screen.getByRole("button", { name: "Validate" }));
    await user.click(await screen.findByRole("button", { name: "Save and apply" }));
    expect(
      await screen.findByRole("alertdialog", { name: "Apply configuration version 8?" }),
    ).toBeTruthy();
    expect(api.validateConfig).toHaveBeenCalledTimes(3);
  });

  it("lists the engine's plan: restarted, updated in place, started, stopped, untouched", async () => {
    const api = stubApi({ validateConfig: async () => ({ ok: true, issues: [], plan: PLAN }) });
    const { user } = renderRoutes(routes, "/channels/chem-bridge/edit", { api });
    await screen.findByRole("heading", { level: 1, name: "Chemistry bridge" });
    await user.click(screen.getByRole("button", { name: "Save and apply" }));
    const dialog = await screen.findByRole("alertdialog");
    const plan = within(dialog).getByRole("list", { name: "Plan" });
    expect(
      within(plan)
        .getAllByRole("listitem")
        .map((item) => item.textContent),
    ).toEqual([
      "Restarted (changed): Chemistry bridge",
      "Updated in place: Results to LIS",
      "Started (new): Billing",
      "Stopped (disabled): Paused feed",
      "Stopped and removed: Old feed",
      "Endpoints changed: LIS host",
      "Alert thresholds change.",
      "Untouched: 2 channels, which keep running.",
    ]);
    expect(
      within(dialog).getByText(/Unresolved deliveries of a removed channel or destination/),
    ).toBeTruthy();
    await typeNote(user);
    await user.click(within(dialog).getByRole("button", { name: "Apply version 8" }));
    await waitFor(() => expect(api.applyConfig).toHaveBeenCalledTimes(1));
  });

  it("shows the warnings of a valid document, with their codes, in the confirmation", async () => {
    const api = stubApi({
      validateConfig: async () => ({
        ok: true,
        plan: NO_PLAN,
        issues: [
          {
            severity: "warning",
            path: "endpoints[0].secretName",
            message: 'No secret named "x" is stored yet.',
            code: "secret_unresolved",
          },
        ],
      }),
    });
    const { user } = renderRoutes(routes, "/channels/chem-bridge/edit", { api });
    await screen.findByRole("heading", { level: 1, name: "Chemistry bridge" });
    await user.click(screen.getByRole("button", { name: "Save and apply" }));
    const dialog = await screen.findByRole("alertdialog");
    expect(within(dialog).getByText('No secret named "x" is stored yet.')).toBeTruthy();
    expect(within(dialog).getByText("endpoints[0].secretName · secret_unresolved")).toBeTruthy();
    expect(within(dialog).getByText(/Untouched: 0 channels/)).toBeTruthy();
  });
});
