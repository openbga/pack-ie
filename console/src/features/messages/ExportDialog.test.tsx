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
import { page, rows, type StubApi, stubApi } from "./fixtures";
import { MessagesPage } from "./MessagesPage";
import { forgetMessagesSession } from "./messagesSession";

const ROUTES = [{ path: "/messages", element: <MessagesPage /> }];

describe("Export on the Messages page", () => {
  beforeEach(() => {
    mockGridLayout();
    forgetMessagesSession();
  });

  async function openExport(api: StubApi, path = "/messages") {
    const rendered = renderRoutes(ROUTES, path, { api });
    await screen.findByRole("grid", { name: "Messages" });
    await rendered.user.click(screen.getByRole("button", { name: "Export…" }));
    return {
      ...rendered,
      dialog: await screen.findByRole("dialog", { name: "Export messages" }),
    };
  }

  const exportButton = (dialog: HTMLElement) =>
    within(dialog).getByRole("button", { name: "Export" });

  it("states the filters in effect and how many messages match, with no violations", async () => {
    const api = stubApi({ listMessages: async () => page(rows(1, 3), 4321) });
    const { dialog } = await openExport(api, "/messages?quick=attention&direction=inbound");
    expect(
      within(dialog).getByText(
        "Filters in effect: received in the last 24 hours, direction Inbound, quick filter Needs attention.",
      ),
    ).toBeTruthy();
    expect(within(dialog).getByText("4,321 messages match.")).toBeTruthy();
    await expectNoA11yViolations(dialog);
  });

  it("warns that the file lists patient identifiers even without content", async () => {
    const { dialog } = await openExport(stubApi());
    expect(within(dialog).getByText("The file lists patient identifiers")).toBeTruthy();
    expect(within(dialog).getByText(/even without the message content/)).toBeTruthy();
    expect(within(dialog).getByText(/Save the file to a protected location/)).toBeTruthy();
    expect(within(dialog).queryByText("The file will contain message content")).toBeNull();
    const content = within(dialog).getByRole("checkbox", {
      name: "Include message content (original bodies)",
    }) as HTMLInputElement;
    expect(content.checked).toBe(false);
  });

  it("exports the current filter as CSV without content by default, and names the file", async () => {
    const api = stubApi({
      listMessages: async () => page(rows(1, 3), 3),
      exportMessages: async () => ({ savedTo: "/home/lab/m.csv", rows: 3 }),
    });
    const { user, dialog } = await openExport(api, "/messages?direction=inbound&state=routed");
    await user.click(exportButton(dialog));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(api.exportMessages).toHaveBeenCalledWith({
      filter: expect.objectContaining({ directions: ["inbound"], states: ["routed"] }),
      format: "csv",
      includeBodies: false,
    });
    const notices = screen.getByRole("region", { name: "Notifications" });
    expect(within(notices).getByText("Messages exported")).toBeTruthy();
    expect(within(notices).getByText("3 messages saved to /home/lab/m.csv.")).toBeTruthy();
  });

  it("includes the original bodies only after the separate choice, which brings a stronger warning", async () => {
    const api = stubApi({
      exportMessages: async () => ({ savedTo: "/home/lab/m.csv", rows: 1 }),
    });
    const { user, dialog } = await openExport(api);
    await user.click(
      within(dialog).getByRole("checkbox", { name: "Include message content (original bodies)" }),
    );
    expect(within(dialog).getByText("The file will contain message content")).toBeTruthy();
    expect(within(dialog).getByText(/can include patient information/)).toBeTruthy();
    expect(within(dialog).getByText(/as a content view/)).toBeTruthy();
    // The warning comes before the button that acts on it.
    const warning = within(dialog).getByText("The file will contain message content");
    expect(
      warning.compareDocumentPosition(exportButton(dialog)) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    await user.click(exportButton(dialog));
    await waitFor(() => expect(api.exportMessages).toHaveBeenCalledTimes(1));
    expect(api.exportMessages).toHaveBeenCalledWith(
      expect.objectContaining({ includeBodies: true }),
    );
    expect(
      await screen.findByText("1 message saved to /home/lab/m.csv, with their content."),
    ).toBeTruthy();
  });

  it("exports JSON lines when chosen", async () => {
    const api = stubApi();
    const { user, dialog } = await openExport(api);
    await chooseJsonLines(user, dialog);
    await user.click(exportButton(dialog));
    await waitFor(() => expect(api.exportMessages).toHaveBeenCalledTimes(1));
    expect(api.exportMessages).toHaveBeenCalledWith(expect.objectContaining({ format: "jsonl" }));
  });

  it("says when no message matches, and still lets the header-only file be saved", async () => {
    const api = stubApi({ listMessages: async () => page([], 0) });
    const { user } = renderRoutes(ROUTES, "/messages", { api });
    // An empty answer has no grid; the toolbar and its Export… remain.
    await screen.findByText("No messages in this time range");
    await user.click(screen.getByRole("button", { name: "Export…" }));
    const dialog = await screen.findByRole("dialog", { name: "Export messages" });
    expect(
      within(dialog).getByText("No messages match: the file would hold only its header."),
    ).toBeTruthy();
  });

  it("closes quietly when the save dialog was cancelled", async () => {
    const api = stubApi({ exportMessages: async () => null });
    const { user, dialog } = await openExport(api);
    await user.click(exportButton(dialog));
    await expectClosedWithoutNotice("Messages exported");
  });

  it("keeps the dialog open with the error when the export fails, and says no partial file is kept", async () => {
    let fail = true;
    const api = stubApi({
      exportMessages: async () => {
        if (fail) throw engineError("internal", "The file could not be written.");
        return { savedTo: "/tmp/a.csv", rows: 3 };
      },
    });
    const { user, dialog } = await openExport(api);
    await user.click(exportButton(dialog));
    const alert = await within(dialog).findByText("The export failed");
    expect(alert).toBeTruthy();
    expect(within(dialog).getByText("The file could not be written.")).toBeTruthy();
    expect(within(dialog).getByText("A partial file is not kept.")).toBeTruthy();
    fail = false;
    await user.click(exportButton(dialog));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("closes with Cancel without exporting", async () => {
    const api = stubApi();
    const { user, dialog } = await openExport(api);
    await cancelExportDialog(user, dialog, api.exportMessages);
  });
});
