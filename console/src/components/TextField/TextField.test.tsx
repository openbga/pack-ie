import { screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderWithProviders } from "@/lib/test/render";
import { TextField } from "./TextField";

function Harness(props: { type?: "text" | "password"; errorMessage?: string }) {
  const [value, setValue] = useState("");
  return (
    <>
      <TextField
        label="Accession"
        name="accession"
        value={value}
        onChange={setValue}
        isRequired
        {...props}
      />
      <output>{value}</output>
    </>
  );
}

describe("TextField", () => {
  it("is labelled, required and editable", async () => {
    const { user, container } = renderWithProviders(<Harness />);
    const input = screen.getByRole("textbox", { name: /Accession/ });
    expect(input.getAttribute("aria-required")).toBe("true");
    await user.type(input, "SYN-000001");
    expect(screen.getByText("SYN-000001", { selector: "output" })).toBeTruthy();
    await expectNoA11yViolations(container);
  });

  it("hides typed characters for passwords", () => {
    renderWithProviders(<Harness type="password" />);
    expect(screen.getByLabelText(/Accession/).getAttribute("type")).toBe("password");
  });

  it("shows its error and marks the input invalid", async () => {
    const { container } = renderWithProviders(<Harness errorMessage="Enter a value." />);
    const input = screen.getByRole("textbox", { name: /Accession/ });
    expect(input.getAttribute("aria-invalid")).toBe("true");
    expect(screen.getByText("Enter a value.")).toBeTruthy();
    // The field marks its label's container, which turns the label red.
    expect(
      screen
        .getByText(/Accession/, { selector: "label" })
        .parentElement?.hasAttribute("data-invalid"),
    ).toBe(true);
    await expectNoA11yViolations(container);
  });
});
