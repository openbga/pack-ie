import { screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderWithProviders } from "@/lib/test/render";
import { Inset } from "./Inset";

describe("Inset", () => {
  it("renders as a group of fields or a list item", async () => {
    const { container } = renderWithProviders(
      <>
        <Inset as="fieldset">
          <legend>Component 1</legend>
          <p>Synthetic fields</p>
        </Inset>
        <ul>
          <Inset as="li">Synthetic hold</Inset>
        </ul>
      </>,
    );
    expect(screen.getByRole("group", { name: "Component 1" })).toBeTruthy();
    expect(screen.getByRole("listitem").textContent).toBe("Synthetic hold");
    await expectNoA11yViolations(container);
  });
});
