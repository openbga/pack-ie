import { screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import { Button } from "@/components/Button";
import { Form } from "@/components/Form";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { engineError } from "@/lib/test/engineError";
import { renderWithProviders } from "@/lib/test/render";
import { Select } from "./Select";

const OPTIONS = [
  { id: "a", label: "Synthetic panel A" },
  { id: "b", label: "Synthetic panel B" },
];

function Harness() {
  const [value, setValue] = useState<string | null>(null);
  return (
    <>
      <Select label="Panel" name="panel" options={OPTIONS} value={value} onChange={setValue} />
      <output>{value ?? "none"}</output>
    </>
  );
}

describe("Select", () => {
  it("chooses an option with the keyboard", async () => {
    const { user, container } = renderWithProviders(<Harness />);
    await expectNoA11yViolations(container);
    await user.tab();
    await user.keyboard("{ArrowDown}");
    await user.click(await screen.findByRole("option", { name: "Synthetic panel B" }));
    expect(screen.getByText("b", { selector: "output" })).toBeTruthy();
  });

  // The invalid border and red placeholder hang on the select's `data-invalid` from React
  // Aria, which lands on the field wrapper rather than the trigger (ticket #4359).
  function expectInvalidEmpty(name: string) {
    const trigger = screen.getByRole("button", { name: new RegExp(name) });
    expect(trigger.parentElement?.hasAttribute("data-invalid")).toBe(true);
    expect(trigger.querySelector("[data-placeholder]")?.textContent).toBe("Select…");
  }

  it("marks an empty required choice invalid with its message", async () => {
    const { container } = renderWithProviders(
      <Select
        label="Received as"
        name="sourceKind"
        options={OPTIONS}
        value={null}
        onChange={() => undefined}
        isRequired
        errorMessage="Choose received as."
      />,
    );
    expectInvalidEmpty("Received as");
    expect(screen.getByText("Choose received as.")).toBeTruthy();
    await expectNoA11yViolations(container);
  });

  it("marks a choice the engine refused invalid", () => {
    const error = engineError("validation", "Check the highlighted fields.", {
      fieldErrors: [{ field: "panel", message: "Choose a panel." }],
    });
    renderWithProviders(
      <Form
        aria-label="Order"
        onSubmit={() => undefined}
        error={error}
        actions={<Button type="submit">Save</Button>}
      >
        <Select label="Panel" name="panel" options={OPTIONS} value={null} onChange={() => {}} />
      </Form>,
    );
    expectInvalidEmpty("Panel");
    expect(screen.getByText("Choose a panel.")).toBeTruthy();
  });
});
