import { screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MockEngineApi } from "@/api/mock";
import type { EngineError, TransmissionReport } from "@/api/types";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { gridRows } from "@/lib/test/grid";
import { mockGridLayout } from "@/lib/test/layout";
import { renderRoutes } from "@/lib/test/router";
import { CHANNEL, engineError, report, row } from "./fixtures";
import { TransmissionPage } from "./TransmissionPage";

const ROUTES = [{ path: "/transmission", element: <TransmissionPage /> }];

let api: MockEngineApi;
beforeEach(() => {
  mockGridLayout();
  api = new MockEngineApi({ latency: "none", messageCount: 500, seed: 7 });
});
afterEach(() => api.dispose());

/** Answers every report with `answer`, and the export with a saved file. */
function answer(answer: TransmissionReport | EngineError) {
  const verify = vi.spyOn(api, "verifyTransmission").mockImplementation(async () => {
    if ("rows" in answer) return answer;
    throw answer;
  });
  const save = vi.spyOn(api, "saveReportFile").mockResolvedValue({ savedTo: "/tmp/report.csv" });
  return { verify, save };
}

async function open(path = `/transmission?channel=${CHANNEL.id}`) {
  const rendered = renderRoutes(ROUTES, path, { api });
  await screen.findByRole("heading", { level: 1, name: "Transmission report" });
  return rendered;
}

async function runReport(path?: string) {
  const rendered = await open(path);
  await rendered.user.click(screen.getByRole("button", { name: "Run report" }));
  return rendered;
}

/** Opens the export dialog of the report shown and presses its Export CSV button. */
async function exportCsv(user: Awaited<ReturnType<typeof open>>["user"]) {
  await user.click(screen.getByRole("button", { name: "Export CSV…" }));
  const dialog = await screen.findByRole("dialog");
  await user.click(within(dialog).getByRole("button", { name: "Export CSV" }));
  return dialog;
}

const deliveryRows = () => gridRows("Deliveries");
const cells = (rowElement: HTMLElement) =>
  within(rowElement)
    .getAllByRole("gridcell")
    .map((cell) => cell.textContent);
const totals = () => within(screen.getByRole("table", { name: "Totals by destination" }));

describe("TransmissionPage form", () => {
  it("asks for a channel first, preselects one from the address, and says the run is audited", async () => {
    const { user } = await open("/transmission");
    expect(
      screen.getByText(/Running a report is recorded in the audit log with your account\./),
    ).toBeTruthy();
    expect(screen.getByText("No report yet")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Run report" }).hasAttribute("disabled")).toBe(true);
    expect(screen.getByText(/Choose a channel\./)).toBeTruthy();
    expect(screen.queryByRole("grid")).toBeNull();
    expect(user).toBeTruthy();
  });

  it("shows the channel of the address chosen, and offers message channels only", async () => {
    const { user } = await open();
    const box = await screen.findByRole("combobox", { name: /Channel/ });
    await waitFor(() => expect((box as HTMLInputElement).value).toContain(CHANNEL.label));
    expect(screen.getByRole("button", { name: "Run report" }).hasAttribute("disabled")).toBe(false);
    await user.click(box);
    const options = await screen.findAllByRole("option");
    expect(options.length).toBeGreaterThan(5);
    // A bridge passes bytes: the mock's bridges are not in the list.
    expect(screen.queryByRole("option", { name: /to middleware/ })).toBeNull();
  });

  it("keeps a channel the configuration does not list choosable, as the engine allows", async () => {
    await open("/transmission?channel=ch-removed");
    const box = await screen.findByRole("combobox", { name: /Channel/ });
    await waitFor(() => expect((box as HTMLInputElement).value).toContain("ch-removed"));
  });

  it("does not run while the form changes, and keeps the form in the address", async () => {
    const { verify } = answer(report());
    const { user, router } = await open();
    await user.click(screen.getByRole("button", { name: /Period$/ }));
    await user.click(await screen.findByRole("option", { name: "Last 7 days" }));
    expect(router.state.location.search).toContain("range=7d");
    expect(verify).not.toHaveBeenCalled();
  });

  it("needs a start for a custom period and offers From and To", async () => {
    const { user } = await open(`/transmission?channel=${CHANNEL.id}&range=custom`);
    expect(screen.getByText(/Choose when the period starts\./)).toBeTruthy();
    expect(screen.getByLabelText("From")).toBeTruthy();
    expect(screen.getByLabelText("To")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Run report" }).hasAttribute("disabled")).toBe(true);
    expect(user).toBeTruthy();
  });

  it("refuses an end before the start", async () => {
    await open(
      `/transmission?channel=${CHANNEL.id}&since=2026-11-02T00:00:00Z&until=2026-11-01T00:00:00Z`,
    );
    expect(screen.getByText(/The period must end after it starts\./)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Run report" }).hasAttribute("disabled")).toBe(true);
  });
});

describe("TransmissionPage report", () => {
  it("runs the report for the channel and the last 24 hours, and says what it covers", async () => {
    const { verify } = answer(report());
    const before = Date.now();
    const { container } = await runReport();
    await screen.findByRole("grid", { name: "Deliveries" });
    expect(verify).toHaveBeenCalledTimes(1);
    const query = verify.mock.calls[0]?.[0];
    expect(query?.channelId).toBe(CHANNEL.id);
    expect(query?.until).toBeUndefined();
    expect(Math.abs(Date.parse(query?.since ?? "") - (before - 86_400_000))).toBeLessThan(5000);
    const facts = within(screen.getByRole("region", { name: "Report" }));
    expect(facts.getByText(CHANNEL.label)).toBeTruthy();
    expect(facts.getByText("Messages received")).toBeTruthy();
    expect(facts.getByText("3")).toBeTruthy();
    await expectNoA11yViolations(container);
  });

  it("totals every destination with the spec's words for what was proven", async () => {
    answer(report());
    await runReport();
    await screen.findByRole("table", { name: "Totals by destination" });
    const headers = totals()
      .getAllByRole("columnheader")
      .map((header) => header.textContent);
    expect(headers).toEqual([
      "Destination",
      "Deliveries",
      "Accepted",
      "Received",
      "Written, not confirmed",
      "Unresolved",
      "Closed",
      "Pending",
    ]);
    const [lis, archive] = totals().getAllByRole("row").slice(1);
    expect(
      within(lis as HTMLElement)
        .getAllByRole("cell")
        .map((c) => c.textContent),
    ).toEqual(["4", "2", "0", "1", "1", "0", "0"]);
    expect(within(archive as HTMLElement).getByRole("rowheader").textContent).toBe("Archive file");
    expect(screen.queryByText(/\bdelivered\b/i, { selector: "th" })).toBeNull();
  });

  it("lists each delivery with its control id, hash, ack level, ack code and outcome", async () => {
    answer(report());
    await runReport();
    await screen.findByRole("grid", { name: "Deliveries" });
    const headers = within(screen.getByRole("grid", { name: "Deliveries" }))
      .getAllByRole("columnheader")
      .map((header) => header.textContent);
    expect(headers).toEqual([
      "Received",
      "Message",
      "Control ID",
      "Content hash",
      "Destination",
      "Ack level",
      "Ack code",
      "ACK ID",
      "Acknowledged at",
      "Outcome",
    ]);
    const rows = deliveryRows();
    expect(rows).toHaveLength(5);
    const [accepted, received, written, timedOut] = rows.map(cells);
    expect(accepted?.slice(1)).toEqual([
      "msg-1",
      "CTL-1",
      `${"a".repeat(12)}…`,
      "LIS (HL7)",
      "Accepted",
      "AA",
      "ACK-1",
      expect.stringMatching(/^\d\d-\d\d \d\d:\d\d:\d\d$/),
      "Acknowledged",
    ]);
    expect(received?.[5]).toBe("Received");
    expect(received?.[6]).toBe("CA");
    expect(written?.[5]).toBe("Written, not confirmed");
    expect(written?.[6]).toBe("None");
    expect(accepted?.[7]).toBe("ACK-1");
    // No acknowledgement at all is never worded as delivered or successful.
    expect(timedOut?.[2]).toBe("Not provided");
    expect(timedOut?.[5]).toBe("None");
    expect(timedOut?.[8]).toBe("Not acknowledged");
    expect(timedOut?.[9]).toBe("No acknowledgement in time, may have been delivered");
    // The whole hash is the title; the message links to its record.
    expect(within(rows[0] as HTMLElement).getByTitle("a".repeat(64))).toBeTruthy();
    expect(
      within(rows[0] as HTMLElement)
        .getByRole("link", { name: "msg-1" })
        .getAttribute("href"),
    ).toBe("/messages/msg-1");
  });

  it("adds sent hash, attempts and first attempt on request", async () => {
    answer(report());
    const { user } = await runReport();
    await screen.findByRole("grid", { name: "Deliveries" });
    await user.click(screen.getByRole("checkbox", { name: /Show sent hash/ }));
    const headers = within(screen.getByRole("grid", { name: "Deliveries" }))
      .getAllByRole("columnheader")
      .map((header) => header.textContent);
    expect(headers.slice(-3)).toEqual(["Sent hash", "Attempts", "First attempt"]);
    const [first, second, , third] = deliveryRows().map(cells);
    expect(first?.slice(-3, -1)).toEqual([`${"b".repeat(12)}…`, "1"]);
    expect(second?.at(-3)).toBe("None");
    expect(third?.at(-2)).toBe("3");
  });

  it("adds the source endpoint and peer and the configuration and mapping versions on request", async () => {
    answer(
      report({
        rows: [row(1), row(2, { sourcePeer: null, mappingVersion: null, configVersion: 9 })],
        periodRowsTotal: 2,
      }),
    );
    const { user } = await runReport();
    await screen.findByRole("grid", { name: "Deliveries" });
    await user.click(screen.getByRole("checkbox", { name: /Show source and versions/ }));
    const headers = within(screen.getByRole("grid", { name: "Deliveries" }))
      .getAllByRole("columnheader")
      .map((header) => header.textContent);
    expect(headers.slice(-4)).toEqual([
      "Source endpoint",
      "Source peer",
      "Config version",
      "Mapping version",
    ]);
    const [first, second] = deliveryRows().map(cells);
    expect(first?.slice(-4)).toEqual(["ep-analyzer", "10.0.4.17:41234", "12", "12"]);
    expect(second?.slice(-4)).toEqual(["ep-analyzer", "Not recorded", "9", "No mapping"]);
  });

  it("narrows the grid to a destination but not the totals, and keeps the choice in the address", async () => {
    answer(report());
    const { user, router } = await runReport();
    await screen.findByRole("grid", { name: "Deliveries" });
    await user.click(screen.getByRole("button", { name: /Destination$/ }));
    await user.click(await screen.findByRole("option", { name: "Archive file" }));
    await waitFor(() => expect(deliveryRows()).toHaveLength(1));
    expect(cells(deliveryRows()[0] as HTMLElement)[4]).toBe("Archive file");
    expect(screen.getByText("1 of 5 deliveries")).toBeTruthy();
    expect(totals().getAllByRole("row")).toHaveLength(3);
    expect(router.state.location.search).toContain("destination=dest-archive");
    await user.click(screen.getByRole("button", { name: /Destination$/ }));
    await user.click(await screen.findByRole("option", { name: "All destinations" }));
    await waitFor(() => expect(deliveryRows()).toHaveLength(5));
    expect(router.state.location.search).not.toContain("destination");
  });

  it("opens with the destination of the address already chosen", async () => {
    answer(report());
    await runReport(`/transmission?channel=${CHANNEL.id}&destination=dest-archive`);
    await waitFor(() => expect(deliveryRows()).toHaveLength(1));
  });

  it("is told about a destination of the address the report does not list, and shows all rows", async () => {
    answer(report());
    await runReport(`/transmission?channel=${CHANNEL.id}&destination=dest-gone`);
    await waitFor(() => expect(deliveryRows()).toHaveLength(5));
  });
});

describe("TransmissionPage export", () => {
  it("saves exactly the rows shown as CSV, names the file, and says what the file holds", async () => {
    const { save } = answer(report());
    const { user } = await runReport(`/transmission?channel=${CHANNEL.id}&destination=dest-lis`);
    await waitFor(() => expect(deliveryRows()).toHaveLength(4));
    await user.click(screen.getByRole("button", { name: "Export CSV…" }));
    const dialog = await screen.findByRole("dialog", { name: "Export transmission report" });
    expect(within(dialog).getByText("4 deliveries for LIS (HL7).")).toBeTruthy();
    expect(
      within(dialog).getByText(/control id.*patient.*hashes.*no message content/s),
    ).toBeTruthy();
    await user.click(within(dialog).getByRole("button", { name: "Export CSV" }));
    await waitFor(() => expect(save).toHaveBeenCalledTimes(1));
    const file = save.mock.calls[0]?.[0];
    expect(file?.name).toBe("packie-transmission-ch-msg-sta-2026-11-07.csv");
    expect(file?.format).toBe("csv");
    const lines = file?.content.split("\r\n").slice(0, -1) ?? [];
    expect(lines).toHaveLength(5);
    expect(lines.slice(1).every((line) => line.includes(",dest-lis,"))).toBe(true);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(await screen.findByText("4 deliveries saved to /tmp/report.csv.")).toBeTruthy();
  });

  it("warns in the export dialog that a truncated report is incomplete, and the file says so", async () => {
    const rows = Array.from({ length: 10_000 }, (_, n) => row(n));
    const { save } = answer(
      report({ truncated: true, rows, messages: 12_000, periodRowsTotal: 51_651 }),
    );
    const { user } = await runReport();
    await user.click(await screen.findByRole("button", { name: "Export CSV…" }));
    const dialog = await screen.findByRole("dialog", { name: "Export transmission report" });
    const warning = within(dialog).getByRole("alert");
    expect(within(warning).getByText("This report is incomplete")).toBeTruthy();
    expect(
      within(warning).getByText(/only the first 10,000 of 51,651 are in the\s+report/),
    ).toBeTruthy();
    expect(within(warning).getByText(/line starting “# truncated:”/)).toBeTruthy();
    expect(within(warning).getByText(/report a shorter period/)).toBeTruthy();
    await user.click(within(dialog).getByRole("button", { name: "Export CSV" }));
    await waitFor(() => expect(save).toHaveBeenCalledTimes(1));
    const tail = save.mock.calls[0]?.[0].content.trimEnd().split("\r\n").slice(-2);
    expect(tail?.[0]).toMatch(/^# listed: 10000 of 51651 deliveries/);
    expect(tail?.[1]).toMatch(/^# truncated:/);
  });

  it("warns of nothing in the export dialog for a complete report", async () => {
    answer(report());
    const { user } = await runReport();
    await user.click(await screen.findByRole("button", { name: "Export CSV…" }));
    const dialog = await screen.findByRole("dialog", { name: "Export transmission report" });
    expect(within(dialog).queryByText("This report is incomplete")).toBeNull();
  });

  it("is silent when the save dialog was cancelled", async () => {
    const { save } = answer(report());
    save.mockResolvedValue(null);
    const { user } = await runReport();
    await screen.findByRole("grid", { name: "Deliveries" });
    await exportCsv(user);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    const notices = screen.getByRole("region", { name: "Notifications" });
    expect(within(notices).queryByRole("listitem")).toBeNull();
  });

  it("keeps the dialog open with the reason when the file cannot be saved", async () => {
    const { save } = answer(report());
    save.mockRejectedValue(engineError("internal", "The file could not be written."));
    const { user } = await runReport();
    await screen.findByRole("grid", { name: "Deliveries" });
    const dialog = await exportCsv(user);
    expect(await within(dialog).findByText("The report could not be saved")).toBeTruthy();
    expect(within(dialog).getByText("The file could not be written.")).toBeTruthy();
  });

  it("cannot export an empty narrowing", async () => {
    answer(report({ destinations: [...report().destinations], rows: [row(1)] }));
    const { user } = await runReport(
      `/transmission?channel=${CHANNEL.id}&destination=dest-archive`,
    );
    expect(await screen.findByText("No deliveries to list")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Export CSV…" }).hasAttribute("disabled")).toBe(true);
    await user.click(screen.getByRole("button", { name: "Show all destinations" }));
    await waitFor(() => expect(deliveryRows()).toHaveLength(1));
  });
});

describe("TransmissionPage states", () => {
  it("says so when no message was received in the period", async () => {
    answer(report({ messages: 0, destinations: [], rows: [] }));
    await runReport();
    expect(await screen.findByText("No messages were received in this period")).toBeTruthy();
    expect(screen.queryByRole("grid")).toBeNull();
    expect(screen.queryByRole("table")).toBeNull();
  });

  it("explains messages that created no delivery", async () => {
    answer(report({ messages: 2, destinations: [], rows: [] }));
    await runReport();
    expect(await screen.findByText("No deliveries to list")).toBeTruthy();
    expect(screen.getByText("No deliveries were created in this period.")).toBeTruthy();
  });

  it("warns that a truncated report is incomplete and asks for a shorter period", async () => {
    const rows = Array.from({ length: 10_000 }, (_, n) => row(n % 50));
    answer(report({ truncated: true, rows, messages: 12_000, periodRowsTotal: 51_651 }));
    await runReport();
    const alert = await screen.findByRole("alert");
    expect(
      within(alert).getByText(
        /Only the first 10,000 of 51,651 deliveries are listed\. Choose a shorter period to list every one\./,
      ),
    ).toBeTruthy();
    // The engine counts the whole period; only the rows stop early.
    expect(within(alert).getByText(/totals below cover the whole period/)).toBeTruthy();
  });

  it("shows no warning for a complete report", async () => {
    answer(report());
    await runReport();
    await screen.findByRole("grid", { name: "Deliveries" });
    expect(screen.queryByText(/Only the first/)).toBeNull();
  });

  it("shows a loading state while the report runs", async () => {
    let finish: (value: TransmissionReport) => void = () => undefined;
    vi.spyOn(api, "verifyTransmission").mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    await runReport();
    expect(await screen.findByText("Running the report…")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Run report" }).hasAttribute("data-pending")).toBe(
      true,
    );
    finish(report());
    await screen.findByRole("grid", { name: "Deliveries" });
    expect(screen.queryByText("Running the report…")).toBeNull();
  });

  it("shows the error with its reference, and Try again runs the same report", async () => {
    const { verify } = answer(engineError("not_found", "Channel ch-msg-sta was not found."));
    const { user } = await runReport();
    const alert = await screen.findByRole("alert");
    expect(within(alert).getByText("The report could not be run")).toBeTruthy();
    expect(within(alert).getByText(/corr-1/)).toBeTruthy();
    expect(screen.queryByRole("grid")).toBeNull();
    verify.mockResolvedValue(report());
    await user.click(within(alert).getByRole("button", { name: "Try again" }));
    await screen.findByRole("grid", { name: "Deliveries" });
    expect(verify).toHaveBeenCalledTimes(2);
    expect(verify.mock.calls[1]?.[0]).toEqual(verify.mock.calls[0]?.[0]);
  });
});

describe("TransmissionPage against the mock engine", () => {
  it("reports what the mock holds, and its totals add up with its rows", async () => {
    await runReport("/transmission?channel=ch-msg-sta&range=7d");
    await screen.findByRole("table", { name: "Totals by destination" });
    const body = totals().getAllByRole("row").slice(1);
    const deliveries = body.reduce(
      (sum, each) =>
        sum + Number(within(each).getAllByRole("cell")[0]?.textContent?.replace(",", "")),
      0,
    );
    expect(deliveries).toBeGreaterThan(0);
    const count = screen.getByText(/^[\d,]+ deliver(y|ies)$/).textContent ?? "";
    expect(Number(count.split(" ")[0]?.replace(",", ""))).toBe(deliveries);
  });
});
