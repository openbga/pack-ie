import { screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { mockGridLayout } from "@/lib/test/layout";
import { renderRoutes } from "@/lib/test/router";
import { engineFailure } from "./testFixtures";
import { documents, history, versionRoutes as routes } from "./testVersions";

describe("VersionHistory: Export change report", () => {
  beforeEach(mockGridLayout);

  const CONNECTED = {
    state: "connected" as const,
    endpoint: null,
    detail: null,
    since: "2026-11-06T08:00:00Z",
    actor: "LAB\\jdoe",
  };

  async function openExport(overrides = {}, route = "/settings/versions") {
    const api = history({
      getConnection: async () => CONNECTED,
      saveReportFile: vi.fn(async () => ({ savedTo: "C:\\Reports\\change.csv" })),
      ...overrides,
    });
    const rendered = renderRoutes(routes, route, { api });
    await rendered.user.click(await screen.findByRole("button", { name: "Export change report…" }));
    const dialog = await screen.findByRole("dialog", { name: "Export change report" });
    return { ...rendered, api, dialog };
  }

  it("says what the file holds and how the comparison reads, with no violations", async () => {
    const { dialog } = await openExport();
    expect(within(dialog).getByText(/Version N is compared with version N-1/)).toBeTruthy();
    expect(within(dialog).getByText(/laboratory's change-control record/)).toBeTruthy();
    // Notes and references are operator-entered text: the file is warned about as the audit export is.
    expect(within(dialog).getByText("The file may contain patient information")).toBeTruthy();
    expect(within(dialog).getByText(/can include patient\s+identifiers/)).toBeTruthy();
    expect(within(dialog).getByText(/lab's protected location/)).toBeTruthy();
    // The engine is told first, with the number of versions, and may refuse.
    expect(within(dialog).getByText(/recorded in the audit\s+log, with your account/)).toBeTruthy();
    await expectNoA11yViolations(dialog);
  });

  it("reads every version, saves a CSV and names the file in a notice", async () => {
    const { user, dialog, api } = await openExport();
    await user.click(within(dialog).getByRole("button", { name: "Export" }));
    expect(await screen.findByText("Change report saved")).toBeTruthy();
    expect(screen.getByText("3 versions saved to C:\\Reports\\change.csv.")).toBeTruthy();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(
      vi
        .mocked(api.getConfigVersion)
        .mock.calls.map(([n]) => n)
        .sort(),
    ).toEqual([1, 2, 3]);
    const [file] = vi.mocked(api.saveReportFile).mock.calls[0] ?? [];
    expect(file?.format).toBe("csv");
    expect(file?.name).toMatch(/^packie-change-report-\d{4}-\d{2}-\d{2}\.csv$/);
    expect(file?.audit).toEqual({ kind: "change_report", rows: 3 });
    expect(file?.content).toContain("kind,version,applied_by");
    expect(file?.content).toContain("by LAB\\jdoe.");
    expect(file?.content).toContain(
      "verification,2,,,,,,,,,,,,LAB\\mlopez,2026-11-04T15:00:00Z,VR-2026-018",
    );
    expect(file?.content).toContain("Delivery queue warning,100,500");
  });

  it("saves JSON of one version when asked", async () => {
    const { user, dialog, api } = await openExport({}, "/settings/versions?version=2");
    await user.click(within(dialog).getByRole("button", { name: /Versions$/ }));
    await user.click(await screen.findByRole("option", { name: "Version 2 only" }));
    await user.click(within(dialog).getByRole("button", { name: /Format$/ }));
    await user.click(await screen.findByRole("option", { name: /^JSON/ }));
    await user.click(within(dialog).getByRole("button", { name: "Export" }));
    expect(await screen.findByText("1 version saved to C:\\Reports\\change.csv.")).toBeTruthy();
    const [file] = vi.mocked(api.saveReportFile).mock.calls[0] ?? [];
    expect(file?.format).toBe("json");
    expect(file?.name).toMatch(/^packie-change-report-v2-.*\.json$/);
    const parsed = JSON.parse(file?.content ?? "");
    expect(parsed.versions.map((v: { version: number }) => v.version)).toEqual([2]);
    expect(parsed.versions[0].comparison.against).toBe(1);
    expect(parsed.generated_by).toBe("LAB\\jdoe");
  });

  it("is silent when the save dialog is cancelled", async () => {
    const { user, dialog } = await openExport({ saveReportFile: vi.fn(async () => null) });
    await user.click(within(dialog).getByRole("button", { name: "Export" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(screen.queryByText("Change report saved")).toBeNull();
  });

  it("keeps the dialog open and says why when a version cannot be read, then succeeds on retry", async () => {
    const docs = documents();
    const getConfigVersion = vi
      .fn()
      .mockRejectedValueOnce(engineFailure("engine_unavailable", "The engine is not running."))
      .mockImplementation(async (version: number) => docs[version]);
    const { user, dialog, api } = await openExport({ getConfigVersion });
    await user.click(within(dialog).getByRole("button", { name: "Export" }));
    expect(await within(dialog).findByText("The change report could not be built")).toBeTruthy();
    expect(api.saveReportFile).not.toHaveBeenCalled();
    await user.click(within(dialog).getByRole("button", { name: "Export" }));
    expect(await screen.findByText("Change report saved")).toBeTruthy();
  });
});
