import { screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { MockEngineApi } from "@/api/mock";
import { openRow } from "@/lib/test/grid";
import { mockGridLayout } from "@/lib/test/layout";
import { renderRoutes } from "@/lib/test/router";
import { ChannelEditPage } from "./ChannelEditPage";
import { EndpointEditPage } from "./EndpointEditPage";
import { pick, typeNote } from "./testActions";
import { VersionHistory } from "./VersionHistory";

const routes = [
  { path: "/endpoints/new", element: <EndpointEditPage /> },
  { path: "/endpoints/:id/edit", element: <EndpointEditPage /> },
  { path: "/channels/:id/edit", element: <ChannelEditPage /> },
  { path: "/settings/versions", element: <VersionHistory /> },
];

describe("configuration flow against the mock engine", () => {
  beforeEach(mockGridLayout);

  it("creates an endpoint: engine refuses a duplicate id, the fix applies, the version appears in history", async () => {
    const api = new MockEngineApi({ latency: "none", messageCount: 500 });
    const before = await api.getConfig();
    const existing = before.endpoints[0];
    if (existing === undefined) throw new Error("the mock has no endpoints");
    const { user, router } = renderRoutes(routes, "/endpoints/new", { api });

    await pick(user, document.body, "Kind", "TCP client");
    await user.type(screen.getByRole("textbox", { name: /^Label/ }), "Scenario LIS");
    await user.type(screen.getByRole("textbox", { name: /^Host/ }), "lis.example.test");
    await user.type(screen.getByRole("textbox", { name: /^Port/ }), "3999");
    const id = screen.getByRole("textbox", { name: /^Id\b/ });
    await user.clear(id);
    await user.type(id, existing.id);

    await user.click(screen.getByRole("button", { name: "Validate" }));
    const results = await screen.findByRole("region", { name: "Validation results" });
    expect(within(results).getByText(/Duplicate endpoint id/)).toBeTruthy();
    await waitFor(() => expect(id.getAttribute("aria-invalid")).toBe("true"));
    // Known errors keep Save and apply off, with the reason in words, and nothing is staged.
    expect(screen.getByRole("button", { name: "Save and apply" })).toHaveProperty("disabled", true);
    expect(within(results).getByText(/Save and apply stays off/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: /^Apply version/ })).toBeNull();
    expect((await api.getConfig()).version).toBe(before.version);

    await user.clear(id);
    await user.type(id, "scenario-lis");
    await user.click(screen.getByRole("button", { name: "Validate" }));
    expect(await screen.findByText(/No problems found|Warnings/)).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Save and apply" }));
    await typeNote(user, "Scenario");
    await user.click(
      await screen.findByRole("button", { name: `Apply version ${before.version + 1}` }),
    );
    await waitFor(() => expect(router.state.location.pathname).toBe("/endpoints/scenario-lis"));
    expect(
      await screen.findByText(`Configuration version ${before.version + 1} applied`),
    ).toBeTruthy();
    expect((await api.getConfig()).endpoints.map((endpoint) => endpoint.id)).toContain(
      "scenario-lis",
    );

    await router.navigate("/settings/versions");
    const grid = await screen.findByRole("grid", { name: "Configuration versions" });
    expect(
      within(grid).getByRole("gridcell", { name: `Version ${before.version + 1}` }),
    ).toBeTruthy();
    await openRow(user, `Version ${before.version}`);
    const detail = await screen.findByRole("region", { name: `Version ${before.version}` });
    const removed = await within(detail).findByRole("region", { name: "Endpoints that differ" });
    expect(removed.textContent).toContain(
      `Removed: Scenario LIS scenario-lis (not in version ${before.version})`,
    );
  });

  it("rolls the endpoint back out through the history", async () => {
    const api = new MockEngineApi({ latency: "none", messageCount: 500 });
    const before = await api.getConfig();
    const doc = { ...before, endpoints: before.endpoints.map((endpoint) => ({ ...endpoint })) };
    await api.applyConfig(doc, "Seed a version");
    const { user } = renderRoutes(routes, `/settings/versions?version=${before.version}`, { api });
    await user.click(await screen.findByRole("button", { name: "Roll back to this version…" }));
    await typeNote(user, "Back out the scenario endpoint");
    await user.click(
      await screen.findByRole("button", { name: `Roll back to version ${before.version}` }),
    );
    expect(await screen.findByText(`Rolled back to version ${before.version}`)).toBeTruthy();
    await waitFor(async () => expect((await api.getConfig()).version).toBe(before.version + 2));
  });
});
