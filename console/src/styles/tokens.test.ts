import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

// The path is relative to the repository root, where the npm scripts run Vitest.
// Docs: docs/governance/ui-standards.md#color. Contrast follows WCAG 2.x relative luminance.
const tokensCss = readFileSync("src/styles/tokens.css", "utf8");
const darkStart = tokensCss.indexOf(':root[data-theme="dark"]');

function declarationsOf(css: string, base = new Map<string, string>()): Map<string, string> {
  const declarations = new Map(base);
  for (const match of css.matchAll(/(--[\w-]+):\s*([^;]+);/g)) {
    const [, name, value] = match;
    if (name !== undefined && value !== undefined) declarations.set(name, value.trim());
  }
  return declarations;
}

// The dark theme overrides the light declarations it names.
const light = declarationsOf(tokensCss.slice(0, darkStart));
const themes = { light, dark: declarationsOf(tokensCss.slice(darkStart), light) };
type Theme = keyof typeof themes;

function resolve(theme: Theme, name: string): string {
  const value = themes[theme].get(name);
  if (value === undefined) throw new Error(`Missing token ${name}`);
  const reference = /^var\((--[\w-]+)\)$/.exec(value);
  return reference?.[1] === undefined ? value : resolve(theme, reference[1]);
}

function channel(hex: string, offset: number): number {
  const c = Number.parseInt(hex.slice(offset, offset + 2), 16) / 255;
  return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
}

function luminance(theme: Theme, token: string): number {
  const hex = resolve(theme, token);
  if (!/^#[0-9a-f]{6}$/i.test(hex)) throw new Error(`${token} is not a 6-digit hex color`);
  return 0.2126 * channel(hex, 1) + 0.7152 * channel(hex, 3) + 0.0722 * channel(hex, 5);
}

function contrast(theme: Theme, a: string, b: string): number {
  const [hi, lo] = [luminance(theme, a), luminance(theme, b)].sort((x, y) => y - x);
  if (hi === undefined || lo === undefined) throw new Error("unreachable");
  return (hi + 0.05) / (lo + 0.05);
}

const surface = "--color-surface";
const page = "--color-surface-sunken";
const rowTint = "--color-accent-subtle";
const tones = ["info", "warning", "success", "danger", "hold", "neutral"] as const;

// [foreground, background, minimum ratio, documented light ratio or null]
const pairs: [string, string, number, number | null][] = [
  ["--color-text", surface, 4.5, 16.6],
  ["--color-text-muted", page, 4.5, 6.3],
  ["--color-text-muted", rowTint, 4.5, 5.8],
  ["--color-border-control", surface, 3, 4.7],
  ["--color-accent", surface, 4.5, 6.7],
  ["--color-text-inverse", "--color-accent", 4.5, 6.7],
  ["--color-text-inverse", "--color-accent-hover", 4.5, 8.7],
  ["--color-text", rowTint, 4.5, 14.3],
  ["--color-focus", surface, 3, 6.7],
  ["--color-focus", rowTint, 3, 5.8],
  ["--tone-danger-text", surface, 4.5, null],
  ["--tone-danger-text", "--color-surface-hover", 4.5, null],
  ["--color-text-inverse", "--tone-hold-solid", 4.5, null],
  ["--color-text-inverse", "--tone-danger-solid", 4.5, null],
  ...tones.map((t): [string, string, number, number | null] => [
    `--tone-${t}-text`,
    `--tone-${t}-bg`,
    6.6,
    null,
  ]),
  ["--color-text-muted", "--color-surface-hover", 4.5, null],
  ["--tone-neutral-text", surface, 4.5, null],
  ["--tone-danger-text", "--tone-danger-bg", 4.5, null],
];

describe("design tokens", () => {
  it("defines the dark theme in a later block than the light theme", () => {
    expect(darkStart).toBeGreaterThan(0);
  });

  describe.each(Object.keys(themes) as Theme[])("%s theme", (theme) => {
    it.each(pairs)("%s on %s is at least %d:1", (fg, bg, minimum, documented) => {
      const ratio = contrast(theme, fg, bg);
      expect(ratio).toBeGreaterThanOrEqual(minimum);
      if (theme === "light" && documented !== null) {
        expect(Math.abs(ratio - documented)).toBeLessThanOrEqual(0.1);
      }
    });

    it.each(["info", "warning", "success"] as const)(
      "the %s border is at least 3:1 on the surface",
      (t) => {
        expect(contrast(theme, `--tone-${t}-border`, surface)).toBeGreaterThanOrEqual(3);
      },
    );

    it.each(["danger", "hold"] as const)(
      "the %s solid fill is at least 3:1 on the surface",
      (t) => {
        expect(contrast(theme, `--tone-${t}-solid`, surface)).toBeGreaterThanOrEqual(3);
        expect(resolve(theme, `--tone-${t}-border`)).toBe(resolve(theme, `--tone-${t}-solid`));
      },
    );

    it("uses the accent for the focus ring", () => {
      expect(resolve(theme, "--color-focus")).toBe(resolve(theme, "--color-accent"));
    });
  });

  it("keeps the paper surface the same in both themes", () => {
    expect(resolve("dark", "--color-neutral-0")).toBe("#ffffff");
  });
});
