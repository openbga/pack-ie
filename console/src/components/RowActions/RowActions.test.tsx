import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderWithProviders } from "@/lib/test/render";
import type { ActionItem } from "./ActionMenu";
import { RowActions } from "./RowActions";

const items = (onAction: (id: string) => void): ActionItem[] => [
  { id: "verify", label: "Verify", onAction: () => onAction("verify") },
  { id: "retire", label: "Retire", onAction: () => onAction("retire"), variant: "danger" },
];

describe("RowActions", () => {
  it("is named for its row, stays out of the Tab order and runs the chosen action", async () => {
    const onAction = vi.fn();
    const { user, container } = renderWithProviders(
      <RowActions identifier="SYN-000001" items={items(onAction)} />,
    );
    const trigger = screen.getByRole("button", { name: "Actions for SYN-000001" });
    expect(trigger.getAttribute("tabindex")).toBe("-1");
    await expectNoA11yViolations(container);
    await user.click(trigger);
    const menu = await screen.findByRole("menu");
    await expectNoA11yViolations(menu);
    await waitFor(() =>
      expect(document.activeElement).toBe(within(menu).getByRole("menuitem", { name: "Verify" })),
    );
    await user.keyboard("{ArrowDown}{Enter}");
    expect(onAction).toHaveBeenCalledWith("retire");
    expect(screen.queryByRole("menu")).toBeNull();
  });
});
