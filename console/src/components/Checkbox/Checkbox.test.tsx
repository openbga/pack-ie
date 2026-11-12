import { screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderWithProviders } from "@/lib/test/render";
import { Checkbox } from "./Checkbox";

function Harness() {
  const [checked, setChecked] = useState(false);
  return (
    <Checkbox isSelected={checked} onChange={setChecked}>
      Print labels
    </Checkbox>
  );
}

describe("Checkbox", () => {
  it("toggles with click and Space", async () => {
    const { user, container } = renderWithProviders(<Harness />);
    const checkbox = screen.getByRole("checkbox", { name: "Print labels" });
    await user.click(checkbox);
    expect((checkbox as HTMLInputElement).checked).toBe(true);
    await user.keyboard(" ");
    expect((checkbox as HTMLInputElement).checked).toBe(false);
    await expectNoA11yViolations(container);
  });

  it("reports the mixed state", () => {
    renderWithProviders(
      <Checkbox isSelected={false} isIndeterminate onChange={() => undefined}>
        Some selected
      </Checkbox>,
    );
    const checkbox = screen.getByRole("checkbox", { name: "Some selected" });
    expect((checkbox as HTMLInputElement).indeterminate).toBe(true);
  });
});
