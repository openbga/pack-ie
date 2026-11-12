import { screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Button } from "@/components/Button";
import { TextField } from "@/components/TextField";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderWithProviders } from "@/lib/test/render";
import { Toolbar } from "./Toolbar";

describe("Toolbar", () => {
  it("shows its actions and the count after them", async () => {
    const onPress = vi.fn();
    const { user, container } = renderWithProviders(
      <Toolbar hint="3 selected">
        <Button onPress={onPress}>Open</Button>
      </Toolbar>,
    );
    await user.click(screen.getByRole("button", { name: "Open" }));
    expect(onPress).toHaveBeenCalledOnce();
    expect(container.textContent).toBe("Open3 selected");
    await expectNoA11yViolations(container);
  });

  it("shows no hint when none is given", () => {
    const { container } = renderWithProviders(
      <Toolbar>
        <Button>Open</Button>
      </Toolbar>,
    );
    expect(container.textContent).toBe("Open");
  });

  it("discloses advanced filters, preserves applied feedback and returns focus on Escape", async () => {
    const { user, container } = renderWithProviders(
      <Toolbar
        variant="collection"
        count="12 of 28 records"
        actions={<Button>Refresh</Button>}
        moreFilters={{
          content: (
            <TextField name="code" label="Record code" value="SYN" onChange={() => undefined} />
          ),
          applied: "Record code: SYN",
        }}
      >
        <Button>Common filter</Button>
      </Toolbar>,
    );
    const toggle = screen.getByRole("button", { name: "More filters" });
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByRole("textbox", { name: "Record code" })).toBeNull();
    expect(screen.getByText("Record code: SYN")).toBeTruthy();
    expect(screen.getByRole("status").textContent).toBe("12 of 28 records");
    expect(screen.getAllByRole("button").map((button) => button.textContent)).toEqual([
      "Common filter",
      "More filters",
      "Refresh",
    ]);
    await user.click(toggle);
    const field = screen.getByRole("textbox", { name: "Record code" });
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(
      document.getElementById(toggle.getAttribute("aria-controls") ?? "")?.contains(field),
    ).toBe(true);
    await user.click(field);
    await user.keyboard("{Escape}");
    expect(document.activeElement).toBe(toggle);
    expect(screen.queryByRole("textbox", { name: "Record code" })).toBeNull();
    expect(screen.getByText("Record code: SYN")).toBeTruthy();
    await expectNoA11yViolations(container);
  });

  it("omits an unavailable count while preserving a known empty count", () => {
    const { rerender } = renderWithProviders(<Toolbar count={false} />);
    expect(screen.queryByRole("status")).toBeNull();
    rerender(<Toolbar count="0 records" />);
    expect(screen.getByRole("status").textContent).toBe("0 records");
  });
});
