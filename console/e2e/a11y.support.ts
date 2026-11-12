import { expect, type Page } from "@playwright/test";

interface AxeScope {
  /** A selector for the part of the page to check (default: the whole document). */
  selector?: string;
  /** Only these axe rules (default: all of them). */
  rules?: string[];
}

/** The axe violations of the page now, as "<rule>: <targets>" lines. */
export async function axeViolations(page: Page, scope: AxeScope = {}): Promise<string[]> {
  // Playwright runs in this package's directory; axe-core is one of its dependencies.
  await page.addScriptTag({ path: "node_modules/axe-core/axe.min.js" });
  return page.evaluate(async ({ selector, rules }) => {
    const axe = (
      window as unknown as {
        axe: {
          run: (
            context: unknown,
            options: unknown,
          ) => Promise<{ violations: { id: string; nodes: { target: unknown[] }[] }[] }>;
        };
      }
    ).axe;
    const context = selector ? document.querySelector(selector) : document;
    const results = await axe.run(context, rules ? { runOnly: rules } : {});
    return results.violations.map(
      (each) => `${each.id}: ${each.nodes.map((node) => node.target.join(" ")).join(", ")}`,
    );
  }, scope);
}

/** Fails with the violations listed when axe finds any in `scope`. */
export async function expectNoAxeViolations(page: Page, scope: AxeScope = {}): Promise<void> {
  expect(await axeViolations(page, scope)).toEqual([]);
}
