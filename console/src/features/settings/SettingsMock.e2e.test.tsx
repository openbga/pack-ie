import { screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { MockEngineApi } from "@/api/mock";
import { openRow } from "@/lib/test/grid";
import { mockGridLayout } from "@/lib/test/layout";
import { renderRoutes } from "@/lib/test/router";
import { settingsRoutes } from "./testFixtures";

describe("settings against the mock engine", () => {
  beforeEach(mockGridLayout);

  it("reads every tab from the engine", async () => {
    const api = new MockEngineApi({ latency: "none", messageCount: 500 });
    const { user } = renderRoutes(settingsRoutes, "/settings/engine", { api });
    expect(await screen.findByText(/read when the engine starts/)).toBeTruthy();
    expect(await screen.findByRole("textbox", { name: /Delivery queue warning/ })).toBeTruthy();
    for (const [tab, grid] of [
      ["Retention", "Retention by channel"],
      ["Secrets", "Secrets"],
      ["Versions", "Configuration versions"],
    ] as const) {
      await user.click(screen.getByRole("tab", { name: tab }));
      expect(await screen.findByRole("grid", { name: grid })).toBeTruthy();
    }
    await user.click(screen.getByRole("tab", { name: "About" }));
    expect(await screen.findByText("AGPL-3.0-only")).toBeTruthy();
  });

  it("changes a channel's retention and a secret's value", async () => {
    const api = new MockEngineApi({ latency: "none", messageCount: 500 });
    const config = await api.getConfig();
    const channel = config.channels.find((each) => each.kind === "message");
    if (channel === undefined) throw new Error("the mock has no message channel");
    const { user } = renderRoutes(settingsRoutes, "/settings/retention", { api });
    await screen.findByRole("grid", { name: "Retention by channel" });
    await openRow(user, channel.label);
    const dialog = await screen.findByRole("dialog");
    const days = within(dialog).getByRole("textbox", { name: /Keep for/ });
    await user.clear(days);
    await user.type(days, "45");
    await user.type(within(dialog).getByRole("textbox", { name: /^Change note/ }), "Keep 45 days");
    await user.click(within(dialog).getByRole("button", { name: "Save retention" }));
    expect(await screen.findByText(/Configuration version \d+ applied/)).toBeTruthy();
    const after = (await api.getConfig()).channels.find((each) => each.id === channel.id);
    expect(after?.kind === "message" && after.retention.days).toBe(45);

    await user.click(screen.getByRole("tab", { name: "Secrets" }));
    await user.click(await screen.findByRole("button", { name: "Add secret" }));
    const form = await screen.findByRole("dialog", { name: "Add secret" });
    await user.type(within(form).getByRole("textbox", { name: /^Name/ }), "e2e-secret");
    await user.type(within(form).getByLabelText(/^Value/), "synthetic-value");
    await user.click(within(form).getByRole("button", { name: "Add secret" }));
    expect(await screen.findByText("Secret e2e-secret saved")).toBeTruthy();
    await waitFor(async () =>
      expect(
        (await api.listSecrets()).find((secret) => secret.name === "e2e-secret")?.hasValue,
      ).toBe(true),
    );
    expect(await screen.findByRole("gridcell", { name: "e2e-secret" })).toBeTruthy();
  });
});
