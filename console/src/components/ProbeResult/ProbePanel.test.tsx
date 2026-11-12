import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { EngineError, ProbeReport } from "@/api/types";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { PASSED_PROBE as passed } from "@/lib/test/probeReport";
import { ProbePanel } from "./ProbePanel";
import { formatMillis, stepWords } from "./probeText";

const failed: ProbeReport = {
  ok: false,
  summary: "The connect step failed: connection refused.",
  steps: [
    { name: "resolve", ok: true, detail: "resolved to 10.0.4.21", millis: 1500 },
    { name: "connect", ok: false, detail: "connection refused", millis: 0 },
  ],
};

const rejected: EngineError = {
  code: "validation",
  message: "The endpoint's settings were rejected before any test ran.",
  fieldErrors: [
    { field: "endpoints[0].port", message: "Port must be a whole number from 1 to 65535." },
  ],
  correlationId: "c-1",
};

function show(props: Partial<Parameters<typeof ProbePanel>[0]> = {}) {
  const onRetry = vi.fn();
  const view = render(
    <ProbePanel
      kind="tcp_client"
      action={<button type="button">Test connection</button>}
      isPending={false}
      report={null}
      error={null}
      onRetry={onRetry}
      {...props}
    />,
  );
  return { ...view, onRetry };
}

describe("ProbePanel", () => {
  it("says what the test does and leaves behind, before any result", () => {
    show();
    expect(
      screen.getByText(
        "The test only opens, connects or reads. No message or instrument data is sent, the configuration is not changed and no test file is left behind. Each test is recorded in the audit log with your account.",
      ),
    ).toBeTruthy();
    expect(screen.getByRole("button", { name: "Test connection" })).toBeTruthy();
    expect(screen.queryByRole("table")).toBeNull();
  });

  it("says a folder test also writes: one empty hidden file, created and deleted again", () => {
    show({ kind: "folder" });
    const text = screen.getByText(/The test reads the folder and counts the files that match/);
    expect(text.textContent).toContain("creates one empty hidden file there and deletes it again");
    expect(text.textContent).toContain("never their names");
    expect(text.textContent).toContain("no test file is left behind");
    // The promise of the other kinds would be untrue here.
    expect(text.textContent).not.toContain("only opens, connects or reads");
  });

  it("shows a passed test: the sentence, then each step with its result in words and its time", async () => {
    const { container } = show({ report: passed });
    expect(screen.getByText("Test passed")).toBeTruthy();
    expect(screen.getByText(passed.summary)).toBeTruthy();
    const rows = within(screen.getByRole("table", { name: "Test steps" })).getAllByRole("row");
    expect(rows).toHaveLength(3);
    expect(within(rows[1] as HTMLElement).getByText("Resolve host")).toBeTruthy();
    expect(within(rows[1] as HTMLElement).getByText("Passed")).toBeTruthy();
    expect(within(rows[1] as HTMLElement).getByText("12 ms")).toBeTruthy();
    expect(
      within(rows[2] as HTMLElement).getByText("Connected to 10.0.4.21:3001 in 14 ms, then closed"),
    ).toBeTruthy();
    await expectNoA11yViolations(container);
  });

  it("shows a failed test with the failing step marked in words and what to check", async () => {
    const { container } = show({ report: failed });
    expect(screen.getByText("Test failed")).toBeTruthy();
    expect(screen.getByText("The connect step failed: connection refused.")).toBeTruthy();
    const rows = within(screen.getByRole("table", { name: "Test steps" })).getAllByRole("row");
    expect(within(rows[1] as HTMLElement).getByText("1.5 s")).toBeTruthy();
    expect(within(rows[2] as HTMLElement).getByText("Failed")).toBeTruthy();
    expect(within(rows[2] as HTMLElement).getByText("under 1 ms")).toBeTruthy();
    expect(within(rows[2] as HTMLElement).getByText(/nothing listens on that port/)).toBeTruthy();
    expect(within(rows[1] as HTMLElement).queryByText(/nothing listens/)).toBeNull();
    await expectNoA11yViolations(container);
  });

  it("explains a port this engine holds as working when a channel runs it", () => {
    show({
      report: {
        ok: false,
        summary: "The open step failed: in use by this engine (channel Chemistry bridge).",
        steps: [
          {
            name: "open",
            ok: false,
            detail: "in use by this engine (channel Chemistry bridge)",
            millis: 0,
          },
        ],
      },
    });
    expect(screen.getByText(/the endpoint is working if a channel is running it/)).toBeTruthy();
  });

  it("shows that the test is running, and no earlier result", () => {
    show({ isPending: true, report: passed });
    expect(screen.getByText("Testing…")).toBeTruthy();
    expect(screen.queryByRole("table")).toBeNull();
  });

  it("asks for the required settings when the form is incomplete", () => {
    show({ isIncomplete: true });
    expect(screen.getByText("Fill in the required settings first")).toBeTruthy();
    expect(screen.getByText(/Nothing was tested/)).toBeTruthy();
  });

  it("marks a result stale when the settings changed since", () => {
    show({ report: passed, isStale: true });
    expect(screen.getByText(/These settings changed since the test\. Run it again/)).toBeTruthy();
    expect(screen.getByRole("table", { name: "Test steps" })).toBeTruthy();
  });

  it("says rejected settings never ran: on the form, or listed when there is no form", () => {
    const { rerender, onRetry } = show({ error: rejected, isOnForm: true });
    expect(screen.getByText("Settings rejected: no test ran")).toBeTruthy();
    expect(screen.getByText(/The problems are marked on the form/)).toBeTruthy();
    expect(screen.queryByText("Port must be a whole number from 1 to 65535.")).toBeNull();
    rerender(
      <ProbePanel
        kind="tcp_client"
        action={null}
        isPending={false}
        report={null}
        error={rejected}
        onRetry={onRetry}
      />,
    );
    expect(screen.getByText("Port must be a whole number from 1 to 65535.")).toBeTruthy();
  });

  it("shows any other error with the support reference and a way to try again", async () => {
    const error: EngineError = {
      code: "engine_unavailable",
      message: "The engine is not running.",
      fieldErrors: [],
      correlationId: "c-9",
    };
    const { onRetry } = show({ error });
    expect(screen.getByText("The test could not run")).toBeTruthy();
    expect(screen.getByText("c-9")).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });
});

describe("probe wording", () => {
  it("names steps in words and formats times", () => {
    expect(stepWords("read directory")).toBe("Read folder");
    expect(stepWords("something new")).toBe("Something new");
    expect([0, 7, 999, 1000, 12_345].map(formatMillis)).toEqual([
      "under 1 ms",
      "7 ms",
      "999 ms",
      "1.0 s",
      "12.3 s",
    ]);
  });
});
