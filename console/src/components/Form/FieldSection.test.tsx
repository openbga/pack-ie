import { screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { TextField } from "@/components/TextField";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderWithProviders } from "@/lib/test/render";
import { FieldGrid } from "./FieldGrid";
import { FieldSection } from "./FieldSection";

describe("FieldSection", () => {
  it("names its group of fields by its visible title", async () => {
    const { container } = renderWithProviders(
      <FieldSection title="Serial settings">
        <p>Match the analyzer.</p>
        <FieldGrid>
          <TextField name="baud" label="Baud rate" value="" onChange={() => {}} />
          <TextField name="stop" label="Stop bits" value="" onChange={() => {}} />
        </FieldGrid>
      </FieldSection>,
    );
    const group = screen.getByRole("group", { name: "Serial settings" });
    expect(within(group).getByText("Match the analyzer.")).toBeTruthy();
    expect(within(group).getAllByRole("textbox")).toHaveLength(2);
    await expectNoA11yViolations(container);
  });
});
