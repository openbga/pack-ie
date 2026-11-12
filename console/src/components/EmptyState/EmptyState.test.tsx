import { screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Button } from "@/components/Button";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderWithProviders } from "@/lib/test/render";
import { EmptyState } from "./EmptyState";

describe("EmptyState", () => {
  it("explains the empty view and offers the next step", async () => {
    const onPress = vi.fn();
    const { user, container } = renderWithProviders(
      <EmptyState
        title="No specimens"
        description="Received specimens appear here."
        action={<Button onPress={onPress}>Refresh</Button>}
      />,
    );
    expect(screen.getByText("No specimens")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Refresh" }));
    expect(onPress).toHaveBeenCalledOnce();
    await expectNoA11yViolations(container);
  });
});
