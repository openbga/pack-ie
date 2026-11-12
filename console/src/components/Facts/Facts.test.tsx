import { screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderWithProviders } from "@/lib/test/render";
import { Facts } from "./Facts";

describe("Facts", () => {
  it("pairs each label with its value", async () => {
    const { container } = renderWithProviders(
      <Facts
        items={[
          { label: "Accession", value: "SYN-000123" },
          { label: "Status", value: <strong>Received</strong> },
        ]}
      />,
    );
    const terms = screen.getAllByRole("term").map((term) => term.textContent);
    const definitions = screen.getAllByRole("definition").map((each) => each.textContent);
    expect(terms).toEqual(["Accession", "Status"]);
    expect(definitions).toEqual(["SYN-000123", "Received"]);
    await expectNoA11yViolations(container);
  });
});
