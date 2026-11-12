import { screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderWithProviders } from "@/lib/test/render";
import { CheckboxGroup } from "./CheckboxGroup";

const OPTIONS = [
  { id: "a", label: "Synthetic panel A", details: ["Screens synthetic drug A."] },
  { id: "b", label: "Synthetic panel B" },
];

function Harness({ errorMessage }: { errorMessage?: string }) {
  const [value, setValue] = useState<string[]>([]);
  return (
    <>
      <CheckboxGroup
        legend="Panels"
        options={OPTIONS}
        value={value}
        onChange={setValue}
        isRequired
        errorMessage={errorMessage}
      />
      <output>{value.join(",")}</output>
    </>
  );
}

describe("CheckboxGroup", () => {
  it("selects and clears options with the keyboard", async () => {
    const { user, container } = renderWithProviders(<Harness />);
    await expectNoA11yViolations(container);
    expect(screen.getByRole("group", { name: "Panels" })).toBeTruthy();
    expect(screen.getByText("Screens synthetic drug A.")).toBeTruthy();
    await user.tab();
    await user.keyboard(" ");
    await user.tab();
    await user.keyboard(" ");
    expect(screen.getByText("a,b", { selector: "output" })).toBeTruthy();
    await user.keyboard(" ");
    expect(screen.getByText("a", { selector: "output" })).toBeTruthy();
  });

  it("describes the group with its error", async () => {
    const { container } = renderWithProviders(<Harness errorMessage="Choose a panel." />);
    const group = screen.getByRole("group", { name: "Panels" });
    expect(group.getAttribute("aria-describedby")).toBe(screen.getByText("Choose a panel.").id);
    expect(group.hasAttribute("data-invalid")).toBe(true);
    await expectNoA11yViolations(container);
  });
});
