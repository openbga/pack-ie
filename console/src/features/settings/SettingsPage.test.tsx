import { screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { openRow } from "@/lib/test/grid";
import { mockGridLayout } from "@/lib/test/layout";
import { renderRoutes } from "@/lib/test/router";
import { settingsApi, settingsRoutes } from "./testFixtures";

describe("SettingsPage", () => {
  beforeEach(mockGridLayout);

  it("shows the tab named in the address and moves between tabs by link", async () => {
    const { user, router } = renderRoutes(settingsRoutes, "/settings/about", {
      api: settingsApi(),
    });
    expect(screen.getByRole("heading", { level: 1, name: "Settings" })).toBeTruthy();
    const tabs = screen.getByRole("tablist", { name: "Settings" });
    expect(
      within(tabs)
        .getAllByRole("tab")
        .map((tab) => tab.textContent),
    ).toEqual([
      "Engine",
      "Retention",
      "Secrets",
      "Versions",
      "Backup",
      "Notifications",
      "Privacy",
      "About",
      "Help",
    ]);
    expect(within(tabs).getByRole("tab", { name: "About" }).getAttribute("aria-selected")).toBe(
      "true",
    );
    expect(await screen.findByText("abc1234")).toBeTruthy();
    expect(screen.getByText("AGPL-3.0-only")).toBeTruthy();
    expect(screen.getByText("macos-aarch64")).toBeTruthy();
    await user.click(within(tabs).getByRole("tab", { name: "Secrets" }));
    expect(router.state.location.pathname).toBe("/settings/secrets");
    expect(await screen.findByRole("grid", { name: "Secrets" })).toBeTruthy();
  });

  it("says so for an unknown tab and links to Engine", async () => {
    renderRoutes(settingsRoutes, "/settings/nope", { api: settingsApi() });
    expect(screen.getByText("This settings page does not exist")).toBeTruthy();
    expect(screen.getByRole("link", { name: "Go to Engine settings" }).getAttribute("href")).toBe(
      "/settings/engine",
    );
  });

  it("has no accessibility violations on the engine and retention tabs", async () => {
    const { container, user } = renderRoutes(settingsRoutes, "/settings/engine", {
      api: settingsApi(),
    });
    await screen.findByRole("textbox", { name: /Delivery queue warning/ });
    await expectNoA11yViolations(container);
    await user.click(screen.getByRole("tab", { name: "Retention" }));
    await screen.findByRole("grid", { name: "Retention by channel" });
    await openRow(user, "Results to LIS");
    await screen.findByRole("dialog", { name: /Edit retention/ });
    await expectNoA11yViolations(document.body);
  });

  it("shows the version history on its tab", async () => {
    renderRoutes(settingsRoutes, "/settings/versions", { api: settingsApi() });
    expect(await screen.findByText("No versions yet")).toBeTruthy();
  });

  it("lists every help topic and shows the chosen one", async () => {
    const { user, router } = renderRoutes(settingsRoutes, "/settings/help", { api: settingsApi() });
    const topics = screen.getByRole("navigation", { name: "Help topics" });
    const links = within(topics).getAllByRole("link");
    expect(links.length).toBeGreaterThan(5);
    const settings = within(topics).getByRole("link", { name: "Settings" });
    await user.click(settings);
    expect(router.state.location.search).toBe("?topic=settings");
    expect(await screen.findByRole("heading", { level: 3, name: "Settings" })).toBeTruthy();
    expect(
      screen.getByText(
        "Engine, retention, secrets, versions, backup, notifications, privacy, about and help.",
      ),
    ).toBeTruthy();
  });
});
