import { defineConfig, devices } from "@playwright/test";

declare const process: { env: Record<string, string | undefined> };

// The journeys of e2e/ run in a real browser against the Vite dev server, with the mock engine
// (`?mock=1`). Nothing needs an engine, the Rust shell or a display. The Tauri window itself is
// checked by hand: e2e/MANUAL.md.
// `PACKIE_E2E_PORT` runs a second suite beside one already using 5174.
const PORT = Number(process.env.PACKIE_E2E_PORT ?? 5174);
// CI (and `PACKIE_E2E_PREVIEW=1`) serves the built console (`npm run build` first): the dev server
// compiles modules on demand, which on a two-core runner pushes journeys past their timeout.
// Journeys tagged `@dev-server` reach into dev-server modules and run only against `npm run dev`.
const PREVIEW =
  process.env.PACKIE_E2E_PREVIEW === undefined
    ? Boolean(process.env.CI)
    : process.env.PACKIE_E2E_PREVIEW === "1";

export default defineConfig({
  testDir: "e2e",
  testMatch: "**/*.spec.ts",
  // CI runs the `@perf` journeys (timing budgets) in a pass of their own, with one worker, so
  // another journey cannot take their CPU on a two-core runner (PACKIE_E2E_PERF=1 selects them).
  grep: process.env.PACKIE_E2E_PERF === "1" ? /@perf/ : undefined,
  grepInvert: PREVIEW
    ? process.env.CI && process.env.PACKIE_E2E_PERF !== "1"
      ? /@dev-server|@perf/
      : /@dev-server/
    : undefined,
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
  // Private-repository runners have two cores.
  workers: process.env.CI ? 2 : undefined,
  reporter: [["list"]],
  // A run on another port keeps its artifacts apart: each run empties its own directory first.
  outputDir: PORT === 5174 ? "test-results" : `test-results-${PORT}`,
  timeout: 60_000,
  expect: { timeout: 10_000 },
  use: {
    baseURL: `http://localhost:${PORT}`,
    viewport: { width: 1280, height: 800 },
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    command: PREVIEW
      ? `npx vite preview --port ${PORT} --strictPort`
      : `npm run dev -- --port ${PORT}`,
    url: `http://localhost:${PORT}`,
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
