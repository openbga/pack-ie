import axe from "axe-core";
import { expect } from "vitest";

/**
 * Fails with the list of axe violations inside `container`. Components are
 * checked outside page landmarks, so the landmark rule runs only for whole screens.
 */
export async function expectNoA11yViolations(
  container: Element,
  { wholePage = false }: { wholePage?: boolean } = {},
) {
  const results = await axe.run(container, { rules: { region: { enabled: wholePage } } });
  const violations = results.violations.map(
    (violation) =>
      `${violation.id}: ${violation.help} (${violation.nodes.map((node) => node.target.join(" ")).join(", ")})`,
  );
  expect(violations).toEqual([]);
}
