import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

// Vitest does not apply CSS modules, so these layout rules, which no jsdom test can observe,
// are checked in the source. Paths are relative to the repository root, as in tokens.test.ts.
const css = (path: string) => readFileSync(`src/components/${path}`, "utf8");

/** The declarations of the last rule whose selector is exactly `selector`. */
function rule(source: string, selector: string): string {
  const start = source.lastIndexOf(`\n${selector} {`);
  if (start < 0) throw new Error(`No rule ${selector}`);
  return source.slice(start, source.indexOf("}", start));
}

describe("layering and sticky parts", () => {
  it("keeps each grid's sticky header inside the grid", () => {
    expect(rule(css("DataGrid/DataGrid.module.css"), ".grid")).toContain("isolation: isolate;");
  });

  it("keeps the row actions column in view when a grid scrolls sideways", () => {
    const actions = rule(css("DataGrid/DataGrid.module.css"), ".actionsEnd");
    expect(actions).toContain("position: sticky;");
    expect(actions).toContain("inset-inline-end: 0;");
    expect(actions).toContain("background: inherit;");
    expect(rule(css("DataGrid/DataGrid.module.css"), ".row")).toContain(
      "background: var(--color-surface);",
    );
  });

  it("shows every toast in the top-right corner, below the top bar", () => {
    // `.region` opens the file, so give it the newline `rule` looks for.
    const region = rule(`\n${css("Toast/Toast.module.css")}`, ".region");
    expect(region).toContain("position: fixed;");
    expect(region).toContain("top: calc(var(--space-12) + var(--space-4));");
    expect(region).toContain("right: var(--space-4);");
    expect(region).not.toContain("bottom:");
  });
});
