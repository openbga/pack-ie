import { screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderWithProviders } from "@/lib/test/render";
import { Button } from "./Button";

describe("Button", () => {
  it("runs its action on click and keyboard", async () => {
    const onPress = vi.fn();
    const { user, container } = renderWithProviders(
      <Button variant="primary" onPress={onPress}>
        Save
      </Button>,
    );
    await user.click(screen.getByRole("button", { name: "Save" }));
    await user.keyboard("{Enter}");
    expect(onPress).toHaveBeenCalledTimes(2);
    await expectNoA11yViolations(container);
  });

  it("is a link that looks like a button when given an address", async () => {
    renderWithProviders(<Button href="/channels/new">Add channel</Button>);
    const link = screen.getByRole("link", { name: "Add channel" });
    expect(link.getAttribute("href")).toBe("/channels/new");
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("does not run its action while disabled or pending", async () => {
    const onPress = vi.fn();
    const { user } = renderWithProviders(
      <>
        <Button isDisabled onPress={onPress}>
          Disabled
        </Button>
        <Button isPending onPress={onPress}>
          Pending
        </Button>
      </>,
    );
    await user.click(screen.getByRole("button", { name: "Disabled" }));
    await user.click(screen.getByRole("button", { name: "Pending" }));
    expect(onPress).not.toHaveBeenCalled();
  });

  it("offers the danger variant as an ordinary named button", async () => {
    const onPress = vi.fn();
    const { user, container } = renderWithProviders(
      <Button variant="danger" onPress={onPress}>
        Withdraw report
      </Button>,
    );
    await user.click(screen.getByRole("button", { name: "Withdraw report" }));
    expect(onPress).toHaveBeenCalledTimes(1);
    await expectNoA11yViolations(container);
  });
});
