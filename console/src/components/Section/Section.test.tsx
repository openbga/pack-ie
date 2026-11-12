import { screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Button } from "@/components/Button";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderWithProviders } from "@/lib/test/render";
import { Section } from "./Section";

describe("Section", () => {
  it("is a region named by its h2, with its actions beside the heading", async () => {
    const { container } = renderWithProviders(
      <Section title="Containers" variant="plain" actions={<Button>Print labels</Button>}>
        <p>Synthetic content</p>
      </Section>,
    );
    expect(screen.getByRole("region", { name: "Containers" })).toBeTruthy();
    expect(screen.getByRole("heading", { level: 2, name: "Containers" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Print labels" })).toBeTruthy();
    await expectNoA11yViolations(container);
  });
});
