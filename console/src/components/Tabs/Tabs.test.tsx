import { screen } from "@testing-library/react";
import { useParams } from "react-router";
import { describe, expect, it } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderRoutes } from "@/lib/test/router";
import { Tabs } from "./Tabs";

const TABS = [
  { id: "targets", label: "Targets", href: "/backups/targets" },
  { id: "runs", label: "Runs", href: "/backups/runs" },
  { id: "restore-tests", label: "Restore tests", href: "/backups/restore-tests" },
];

function BackupsPage() {
  const { tab = "targets" } = useParams();
  return (
    <Tabs label="Backups" tabs={TABS} selectedId={tab}>
      <p>Synthetic content of {tab}</p>
    </Tabs>
  );
}

const renderBackups = (path = "/backups/targets") =>
  renderRoutes([{ path: "/backups/:tab", element: <BackupsPage /> }], path);

const tab = (name: string) => screen.getByRole("tab", { name });

describe("Tabs", () => {
  it("selects the tab of the current route and shows its panel", async () => {
    const { container } = renderBackups("/backups/runs");
    expect(tab("Runs").getAttribute("aria-selected")).toBe("true");
    expect(tab("Targets").getAttribute("aria-selected")).toBe("false");
    expect(screen.getByRole("tabpanel", { name: "Runs" }).textContent).toBe(
      "Synthetic content of runs",
    );
    await expectNoA11yViolations(container);
  });

  it("navigates to a tab's route when it is clicked", async () => {
    const { user, router, container } = renderBackups();
    await user.click(tab("Restore tests"));
    expect(router.state.location.pathname).toBe("/backups/restore-tests");
    expect(tab("Restore tests").getAttribute("aria-selected")).toBe("true");
    expect(screen.getByText("Synthetic content of restore-tests")).toBeTruthy();
    // The panel's id follows the newly selected tab.
    const panel = screen.getByRole("tabpanel", { name: "Restore tests" });
    expect(tab("Restore tests").getAttribute("aria-controls")).toBe(panel.id);
    await expectNoA11yViolations(container);
  });

  it("moves focus with the arrow keys and opens a tab only with Enter, keeping focus on it", async () => {
    const { user, router } = renderBackups();
    await user.tab();
    expect(document.activeElement).toBe(tab("Targets"));
    await user.keyboard("{ArrowRight}");
    expect(document.activeElement).toBe(tab("Runs"));
    expect(router.state.location.pathname).toBe("/backups/targets");
    await user.keyboard("{Enter}");
    expect(router.state.location.pathname).toBe("/backups/runs");
    expect(tab("Runs").getAttribute("aria-selected")).toBe("true");
    expect(document.activeElement).toBe(tab("Runs"));
  });
});
