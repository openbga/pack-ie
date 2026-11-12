import { screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import type { EngineError } from "@/api/types";
import { Button } from "@/components/Button";
import { Form } from "@/components/Form";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { engineError } from "@/lib/test/engineError";
import { renderWithProviders } from "@/lib/test/render";
import { FieldList } from "./FieldList";

function Harness({ error = null }: { error?: EngineError | null }) {
  const [items, setItems] = useState<{ id: string }[]>([]);
  return (
    <Form
      aria-label="Requisition"
      onSubmit={() => undefined}
      error={error}
      actions={<Button type="submit">Save</Button>}
    >
      <FieldList
        name="diagnoses"
        legend="Diagnoses"
        emptyText="No diagnoses on the requisition."
        items={items}
        itemLabel={(index) => `Diagnosis ${index + 1}`}
        addLabel="Add diagnosis"
        onAdd={() => setItems((rows) => [...rows, { id: `row-${rows.length}` }])}
        onRemove={(index) => setItems((rows) => rows.filter((_, each) => each !== index))}
        renderItem={(index) => <p>Fields of row {index + 1}</p>}
      />
    </Form>
  );
}

describe("FieldList", () => {
  it("starts empty, adds and removes rows", async () => {
    const { user, container } = renderWithProviders(<Harness />);
    expect(screen.getByText("No diagnoses on the requisition.")).toBeTruthy();
    expect(screen.getByRole("group", { name: "Diagnoses" }).hasAttribute("aria-invalid")).toBe(
      false,
    );
    await user.click(screen.getByRole("button", { name: "Add diagnosis" }));
    await user.click(screen.getByRole("button", { name: "Add diagnosis" }));
    expect(screen.getByRole("group", { name: "Diagnosis 2" })).toBeTruthy();
    await expectNoA11yViolations(container);
    await user.click(screen.getByRole("button", { name: "Remove diagnosis 1" }));
    expect(screen.queryByRole("group", { name: "Diagnosis 2" })).toBeNull();
    expect(screen.getByText("Fields of row 1")).toBeTruthy();
  });

  it("shows the engine's error for the whole list under its legend and takes focus", async () => {
    const message = "Add at least one diagnosis.";
    const error = engineError("validation", "Check the highlighted fields.", {
      fieldErrors: [{ field: "diagnoses", message }],
    });
    const { container } = renderWithProviders(<Harness error={error} />);
    const list = screen.getByRole("group", { name: "Diagnoses" });
    expect(list.getAttribute("aria-invalid")).toBe("true");
    const description = document.getElementById(list.getAttribute("aria-describedby") ?? "");
    expect(description?.textContent).toBe(message);
    // The surrounding form moves focus to the first invalid field: here, the list.
    expect(document.activeElement).toBe(list);
    await expectNoA11yViolations(container);
  });
});
