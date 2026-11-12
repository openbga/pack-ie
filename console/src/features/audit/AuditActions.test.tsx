import { screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { engineError } from "@/lib/test/engineError";
import {
  cancelExportDialog,
  chooseJsonLines,
  expectClosedWithoutNotice,
} from "@/lib/test/exportDialog";
import { mockGridLayout } from "@/lib/test/layout";
import { renderRoutes } from "@/lib/test/router";
import { AuditPage } from "./AuditPage";
import { hashOf, type StubApi, stubApi, verification } from "./fixtures";

const ROUTES = [{ path: "/audit", element: <AuditPage /> }];

async function open(api: StubApi, path = "/audit") {
  const rendered = renderRoutes(ROUTES, path, { api });
  await screen.findByRole("grid", { name: "Audit events" });
  return rendered;
}

describe("Export", () => {
  beforeEach(mockGridLayout);

  async function openExport(api: StubApi, path?: string) {
    const rendered = await open(api, path);
    await rendered.user.click(screen.getByRole("button", { name: "Export…" }));
    return {
      ...rendered,
      dialog: await screen.findByRole("dialog", { name: "Export audit events" }),
    };
  }

  it("says the export is recorded and which filters it carries, with no violations", async () => {
    const { dialog } = await openExport(stubApi(), "/audit?action=channel_pause&actor=jdoe");
    expect(within(dialog).getByText(/The export is recorded in the audit log/)).toBeTruthy();
    expect(
      within(dialog).getByText("Filters in effect: actor jdoe, action Channel paused."),
    ).toBeTruthy();
    await expectNoA11yViolations(dialog);
  });

  it("warns that the file may hold patient information, with or without filters", async () => {
    for (const path of ["/audit", "/audit?action=channel_pause&actor=jdoe"]) {
      const { dialog, unmount } = await openExport(stubApi(), path);
      const warning = within(dialog)
        .getByText("The file may contain patient information")
        .closest("[role=alert]") as HTMLElement;
      expect(warning.textContent).toContain("patient identifiers");
      expect(warning.textContent).toContain("reasons and notes");
      expect(warning.textContent).toContain("lab's protected location");
      expect(within(dialog).getByText(/The export is recorded in the audit log/)).toBeTruthy();
      unmount();
    }
  });

  it("says every event is exported when no filter is set", async () => {
    const { dialog } = await openExport(stubApi());
    expect(within(dialog).getByText("No filters are set: every event is exported.")).toBeTruthy();
  });

  it("exports the current filters as CSV, then names the events and the file in a notice", async () => {
    const api = stubApi({
      exportAudit: async () => ({ savedTo: "/home/lab/audit-2026.csv", rows: 1234 }),
    });
    const { user, dialog } = await openExport(api, "/audit?action=channel_pause&outcome=failed");
    await user.click(within(dialog).getByRole("button", { name: "Export" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(api.exportAudit).toHaveBeenCalledWith({
      format: "csv",
      filter: { action: "channel_pause", outcome: "failed" },
    });
    const notices = screen.getByRole("region", { name: "Notifications" });
    expect(within(notices).getByText("Audit log exported")).toBeTruthy();
    expect(
      within(notices).getByText("1,234 events saved to /home/lab/audit-2026.csv."),
    ).toBeTruthy();
  });

  it("exports JSON lines when chosen", async () => {
    const api = stubApi();
    const { user, dialog } = await openExport(api);
    await chooseJsonLines(user, dialog);
    await user.click(within(dialog).getByRole("button", { name: "Export" }));
    await waitFor(() => expect(api.exportAudit).toHaveBeenCalledTimes(1));
    expect(api.exportAudit).toHaveBeenCalledWith({ format: "json_lines", filter: {} });
  });

  it("shows a pending state while the engine saves", async () => {
    let finish: (value: { savedTo: string; rows: number }) => void = () => undefined;
    const api = stubApi({
      exportAudit: () => new Promise((resolve) => (finish = resolve)),
    });
    const { user, dialog } = await openExport(api);
    await user.click(within(dialog).getByRole("button", { name: "Export" }));
    await waitFor(() =>
      expect(
        within(dialog).getByRole("button", { name: "Export" }).hasAttribute("data-pending"),
      ).toBe(true),
    );
    expect(within(dialog).getByRole("button", { name: "Cancel" }).hasAttribute("disabled")).toBe(
      true,
    );
    finish({ savedTo: "/tmp/a.csv", rows: 1 });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(screen.getByText("1 event saved to /tmp/a.csv.")).toBeTruthy();
  });

  it("closes quietly when the save dialog was cancelled", async () => {
    const api = stubApi({ exportAudit: async () => null });
    const { user, dialog } = await openExport(api);
    await user.click(within(dialog).getByRole("button", { name: "Export" }));
    await expectClosedWithoutNotice("Audit log exported");
  });

  it("keeps the dialog open with an alert when the export fails, and can be tried again", async () => {
    let fail = true;
    const api = stubApi({
      exportAudit: async () => {
        if (fail) throw engineError("internal", "The file could not be written.");
        return { savedTo: "/tmp/a.csv", rows: 3 };
      },
    });
    const { user, dialog } = await openExport(api);
    await user.click(within(dialog).getByRole("button", { name: "Export" }));
    const alert = (await within(dialog).findAllByRole("alert")).find((each) =>
      each.textContent?.includes("The export failed"),
    ) as HTMLElement;
    expect(within(alert).getByText("The export failed")).toBeTruthy();
    expect(within(alert).getByText("The file could not be written.")).toBeTruthy();
    fail = false;
    await user.click(within(dialog).getByRole("button", { name: "Export" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("closes with Cancel without exporting", async () => {
    const api = stubApi();
    const { user, dialog } = await openExport(api);
    await cancelExportDialog(user, dialog, api.exportAudit);
  });
});

describe("Verify chain", () => {
  beforeEach(mockGridLayout);

  const region = () => screen.findByRole("region", { name: "Chain verification" });

  it("shows OK with the counts, the head hash in full and where to compare it", async () => {
    const api = stubApi();
    const { user } = await open(api);
    expect(screen.queryByRole("region", { name: "Chain verification" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Verify chain" }));
    const result = await region();
    expect(within(result).getByText("Chain verified: OK")).toBeTruthy();
    expect(within(result).getByText(hashOf(3))).toBeTruthy();
    expect(within(result).getByText("audit.anchor")).toBeTruthy();
    expect(within(result).getByText(/Verifying is recorded in the audit log/)).toBeTruthy();
    for (const label of ["Files", "Events", "First event", "Last event", "Head hash"]) {
      expect(within(result).getByText(label)).toBeTruthy();
    }
    expect(within(result).queryByText(/retention removed older files/)).toBeNull();
    await expectNoA11yViolations(result);
  });

  it("explains a chain that starts later because retention removed older files", async () => {
    const api = stubApi({
      verifyAudit: async () => verification({ firstSeq: 51, events: 10_000, lastSeq: 10_050 }),
    });
    const { user } = await open(api);
    await user.click(screen.getByRole("button", { name: "Verify chain" }));
    const result = await region();
    expect(within(result).getByText("Chain verified: OK")).toBeTruthy();
    expect(
      within(result).getByText(
        "The chain starts at event 51 because retention removed older files (recorded as audit_prune); the rest is intact.",
      ),
    ).toBeTruthy();
  });

  it("says the chain is broken, with each problem in words, its seq and its file", async () => {
    const api = stubApi({
      verifyAudit: async () =>
        verification({
          ok: false,
          problems: [
            { file: "audit.sqlite3", seq: 7, kind: "hash_mismatch", message: "Hash differs." },
            { file: "audit-2025-000000000001.sqlite3", seq: 9, kind: "broken_link", message: "" },
            { file: "audit.sqlite3", seq: null, kind: "unreadable", message: "Not a database." },
            { file: "audit.sqlite3", seq: 12, kind: "sequence_gap", message: "" },
            { file: "audit.sqlite3", seq: 1, kind: "unexplained_start", message: "" },
          ],
        }),
    });
    const { user } = await open(api);
    await user.click(screen.getByRole("button", { name: "Verify chain" }));
    const result = await region();
    expect(within(result).getByText("Chain broken at seq 7")).toBeTruthy();
    expect(within(result).getByText("5 problems found:")).toBeTruthy();
    const items = within(result).getAllByRole("listitem");
    expect(items.map((item) => item.textContent)).toEqual([
      "An event was edited · seq 7 · audit.sqlite3 · Hash differs.",
      "Events were removed, inserted or reordered · seq 9 · audit-2025-000000000001.sqlite3",
      "A file could not be read · no seq · audit.sqlite3 · Not a database.",
      "Events are missing · seq 12 · audit.sqlite3",
      "The oldest events are missing without a record · seq 1 · audit.sqlite3",
    ]);
    expect(within(result).queryByText("Chain verified: OK")).toBeNull();
  });

  it("dismisses the result and refreshes the list, which now holds the verification", async () => {
    const api = stubApi();
    const { user } = await open(api);
    const before = api.getAuditEvents.mock.calls.length;
    await user.click(screen.getByRole("button", { name: "Verify chain" }));
    const result = await region();
    await waitFor(() => expect(api.getAuditEvents.mock.calls.length).toBeGreaterThan(before));
    await user.click(within(result).getByRole("button", { name: "Dismiss notification" }));
    expect(screen.queryByRole("region", { name: "Chain verification" })).toBeNull();
  });

  it("shows an alert with a retry when verification cannot run", async () => {
    let fail = true;
    const api = stubApi({
      verifyAudit: async () => {
        if (fail) throw engineError("unavailable", "The audit log cannot be read.");
        return verification();
      },
    });
    const { user } = await open(api);
    await user.click(screen.getByRole("button", { name: "Verify chain" }));
    const alert = await screen.findByText("The chain could not be verified");
    expect(alert).toBeTruthy();
    fail = false;
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("Chain verified: OK")).toBeTruthy();
  });
});
