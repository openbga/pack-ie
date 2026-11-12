import { expect, type Locator, type Page, test } from "@playwright/test";
import { COUNT, region } from "./channelPages.support";
import { CHANNELS, fact, lastErrorNote, openChannel } from "./operate.support";
import { heading, main } from "./support";

const cellsOf = (row: Locator) => row.getByRole("cell");

const AGO = /^(just now|\d+ (s|min|h|d) ago)/;
const BYTES = /^[\d.,]+ (B|KB|MB|GB)$/;

/** Label, id, state, session and the configuration versions: the facts every channel page has. */
async function expectIdentity(page: Page, channel: { id: string; label: string }, state: RegExp) {
  // The channel's own facts come first; a message channel's connectors repeat some labels below.
  const top = (term: string) => fact(main(page), term).first();
  await expect(heading(page, channel.label)).toBeVisible();
  await expect(top("Channel id")).toHaveText(channel.id);
  await expect(top("State")).toHaveText(state);
  await expect(top("Session")).toBeVisible();
  await expect(top("Enabled")).toHaveText("Yes");
  await expect(top("Configuration version")).toHaveText(/^\d+/);
  await expect(top("Active configuration")).toHaveText(/^Version \d+$/);
  await expect(top("Pending configuration")).toHaveText(/^(None|Version \d+)$/);
}

/** One side of a bridge in the page's Endpoints table, with everything operator-console.md asks of it. */
async function expectBridgeSide(
  page: Page,
  side: "A" | "B",
  expected: { label: string; state: string; kind: string; failing: boolean },
) {
  const table = page.getByRole("table", { name: "Endpoints of this channel" });
  await expect(table.getByRole("columnheader")).toHaveText([
    "Endpoint",
    "Kind",
    "Active peer",
    "Failure streak",
    "Last activity",
    "Bytes received",
    "Bytes sent",
  ]);
  const row = table.getByRole("row").filter({ hasText: `Side ${side}` });
  const cells = cellsOf(row);
  await expect(cells.nth(0).getByRole("link", { name: expected.label })).toBeVisible();
  await expect(cells.nth(0)).toContainText(expected.state);
  await expect(cells.nth(1)).toHaveText(expected.kind);
  await expect(cells.nth(2)).not.toBeEmpty();
  await expect(cells.nth(3)).toHaveText(expected.failing ? /^[1-9]\d*$/ : "0");
  await expect(cells.nth(4)).toHaveText(AGO);
  await expect(cells.nth(5)).toHaveText(BYTES);
  await expect(cells.nth(6)).toHaveText(BYTES);
}

async function expectTranscriptPanel(page: Page, panes: [string, string]) {
  const panel = region(page, "Live transcript");
  await expect(panel.getByRole("button", { name: "Start live transcript" })).toBeEnabled();
  for (const pane of panes) {
    await expect(panel.getByRole("heading", { level: 3, name: pane })).toBeVisible();
    await expect(panel.getByRole("log", { name: `Transcript ${pane}` })).toBeVisible();
  }
}

test.describe("H-03: bridge channel page", () => {
  test("H-03: a retrying bridge shows its identity, both endpoints, traffic, buffers and retry state", async ({
    page,
  }) => {
    const channel = CHANNELS.c503;
    await openChannel(page, channel);
    await expectIdentity(page, channel, /^Retrying/);
    await expect(fact(main(page), "Kind")).toHaveText("Bridge");
    await expect(fact(main(page), "Session")).toHaveText("Closed");

    // State beside the title, in words.
    await expect(main(page).getByText("Retrying").first()).toBeVisible();
    await expect(page.getByRole("button", { name: "Pause" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Restart" })).toBeVisible();

    // Endpoints A and B: state, kind, peer, failure streak, last activity and bytes.
    await expectBridgeSide(page, "A", {
      label: "cobas c 503 (COM5)",
      state: "Connected",
      kind: "Serial",
      failing: false,
    });
    await expectBridgeSide(page, "B", {
      label: "Middleware feed 2",
      state: "Retrying",
      kind: "TCP client",
      failing: true,
    });

    // Bytes in each direction, and the framed message counts of the tap.
    const traffic = page.getByRole("table", { name: "Bytes by direction" });
    await expect(traffic.getByRole("columnheader")).toHaveText([
      "Direction",
      "Bytes",
      "Framed messages",
    ]);
    const directions = traffic.getByRole("row").filter({ has: page.getByRole("cell") });
    await expect(directions).toHaveCount(2);
    await expect(directions.nth(0)).toContainText("cobas c 503 (COM5) → Middleware feed 2");
    await expect(directions.nth(1)).toContainText("Middleware feed 2 → cobas c 503 (COM5)");
    for (const index of [0, 1]) {
      await expect(cellsOf(directions.nth(index)).nth(1)).toHaveText(BYTES);
      await expect(cellsOf(directions.nth(index)).nth(2)).toHaveText(COUNT);
    }

    // Buffers per direction with their limit and the overflow count.
    const buffers = region(page, "Buffers");
    for (const direction of [
      "cobas c 503 (COM5) → Middleware feed 2",
      "Middleware feed 2 → cobas c 503 (COM5)",
    ]) {
      await expect(buffers.getByRole("meter", { name: direction })).toBeVisible();
    }
    await expect(buffers).toContainText(/\d+% · [\d.]+ KB of 256\.0 KB · [\d,]+ of 1,024 segments/);
    await expect(fact(buffers, "Overflow episodes")).toHaveText("0");

    // Retry count and the next retry, as a countdown.
    await expect(fact(main(page), "Retries")).toHaveText(/^[1-9]\d*$/);
    await expect(fact(main(page), "Next retry")).toHaveText(/^in \d+ s$/);

    // Recent errors, recent events, alerts and the transcript.
    const errors = page.getByRole("table", { name: "Recent errors of this channel" });
    await expect(errors.getByRole("row").nth(1)).toContainText("connect timeout after 5000 ms");
    await expect(page.getByRole("table", { name: "Recent events of this channel" })).toBeVisible();
    const alerts = page.getByRole("list", { name: "Active alerts of this channel" });
    await expect(alerts.getByRole("listitem").first()).toContainText("Warning");
    await expect(alerts).toContainText("Middleware feed 2 is reconnecting");
    await expect(alerts).toContainText("Next step:");
    await expectTranscriptPanel(page, ["A → B", "B → A"]);
    await expect(
      region(page, "Live transcript").getByText("A = cobas c 503 (COM5) · B = Middleware feed 2"),
    ).toBeVisible();
  });

  test("H-03: the last error has its full context: failure streak, time, retry state and next step", async ({
    page,
  }) => {
    await openChannel(page, CHANNELS.c503);
    const note = lastErrorNote(page);
    await expect(note).toBeVisible();
    await expect(note).toContainText("connect timeout after 5000 ms (10.0.4.12:3002)");
    await expect(fact(note, "Failure streak")).toHaveText(/^[1-9]\d*$/);
    await expect(fact(note, "Time")).toContainText(/\d{4}-\d{2}-\d{2} .*\(.+ ago\)/);
    await expect(fact(note, "Retry")).toHaveText(/^Next attempt in \d+ s \(retry [1-9]\d*\)$/);
    await expect(note.getByText("Next step:")).toBeVisible();
    await expect(note).toContainText("Check that the middleware server is reachable.");
  });

  test("H-03: a bridge's last error names the operation, the failing endpoint and the cause", async ({
    page,
  }) => {
    await openChannel(page, CHANNELS.c503);
    const note = lastErrorNote(page);
    await expect(fact(note, "Operation")).toBeVisible();
    await expect(fact(note, "Endpoint")).toContainText("Middleware feed 2");
    await expect(fact(note, "Cause")).toBeVisible();
  });

  test("H-03: a bridge held back by a buffer overflow says so, with the buffer, the count and the next step", async ({
    page,
  }) => {
    const channel = CHANNELS.im1600;
    await openChannel(page, channel);
    await expectIdentity(page, channel, /^Degraded/);
    await expect(fact(main(page), "Session")).toHaveText("Bridged");

    const buffers = region(page, "Buffers");
    await expect(buffers).toContainText("Nearly full");
    await expect(buffers).toContainText(/9\d% · [\d.]+ KB of 256\.0 KB/);
    await expect(fact(buffers, "Overflow episodes")).toHaveText("1");
    await expect(fact(buffers, "Now")).toHaveText(
      "Overflowing: the writer waits, nothing is dropped",
    );

    const note = lastErrorNote(page);
    await expect(note).toContainText(
      "buffer overflow A to B: the receiving side is not keeping up",
    );
    await expect(fact(note, "Time")).toBeVisible();
    await expect(note.getByText("Next step:")).toBeVisible();
    await expect(note).toContainText("Check the receiving system.");
    const alerts = page.getByRole("list", { name: "Active alerts of this channel" });
    await expect(alerts).toContainText("Critical");
    await expect(alerts).toContainText("buffer is filling");
  });

  test("H-03: a bridge stopped after an error shows the stop reason and no retry", async ({
    page,
  }) => {
    const channel = { id: "ch-br-d100", label: "D-100 HbA1c to middleware" };
    await openChannel(page, channel);
    await expectIdentity(page, channel, /^Stopped: error/);
    await expect(fact(main(page), "Stop reason")).toHaveText("Stopped after an error");
    await expect(fact(main(page), "Session")).toHaveText("Closed");
    await expect(page.getByRole("button", { name: "Restart" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Pause" })).toHaveCount(0);
    const note = lastErrorNote(page);
    await expect(fact(note, "Retry")).toHaveText("No retry is scheduled");
    await expect(note).toContainText("Restart the channel.");
  });
});

test.describe("H-03: message channel page", () => {
  test("H-03: shows the source connector and every destination with its queue and delivery facts", async ({
    page,
  }) => {
    const channel = CHANNELS.sta;
    await openChannel(page, channel);
    await expectIdentity(page, channel, /^Degraded/);
    await expect(fact(main(page), "Kind")).toHaveText("Message");
    await expect(fact(main(page), "Protocol").first()).toHaveText("ASTM");

    const source = region(page, "Source connector");
    await expect(source.getByRole("link", { name: "STA Compact coag" })).toBeVisible();
    await expect(source).toContainText("Connected");
    await expect(fact(source, "Protocol")).toHaveText("ASTM");
    await expect(fact(source, "Session")).toHaveText("Receiving");
    await expect(fact(source, "Last received")).toContainText(/just now|ago/);
    for (const counter of ["Received", "Duplicates", "Invalid", "Filtered", "Aborted"]) {
      await expect(fact(source, counter)).toHaveText(COUNT);
    }

    const deliveries = region(page, "Destinations and deliveries");
    await expect(deliveries.getByRole("link", { name: "View messages" })).toBeVisible();
    await expect(
      deliveries.getByRole("link", { name: /^View unresolved deliveries \(14\)$/ }),
    ).toBeVisible();
    await expect(fact(deliveries, "Need attention").first()).toContainText("14 need attention");

    // Both destinations, the healthy one and the failing one, each with its own facts.
    for (const label of ["LIS HL7 out", "LIS HL7 out (backup)"]) {
      const destination = region(page, `Destination ${label}`);
      await expect(destination.getByRole("heading", { level: 3, name: label })).toBeVisible();
      await expect(destination.getByRole("link", { name: label, exact: true })).toBeVisible();
      await expect(fact(destination, "Protocol")).toHaveText("HL7 v2");
      await expect(fact(destination, "Queued")).toHaveText(COUNT);
      await expect(fact(destination, "Sending")).toHaveText(COUNT);
      await expect(fact(destination, "Unresolved")).not.toBeEmpty();
      await expect(fact(destination, "Oldest queued")).toHaveText(AGO);
      await expect(fact(destination, "Next attempt")).not.toBeEmpty();
      await expect(fact(destination, "Last acknowledged")).toHaveText(AGO);
    }
    const healthy = region(page, "Destination LIS HL7 out");
    await expect(healthy).toContainText("Connected");
    await expect(fact(healthy, "Unresolved")).toHaveText("None");
    await expect(healthy.getByText("Last error")).toHaveCount(0);

    const failing = region(page, "Destination LIS HL7 out (backup)");
    await expect(failing).toContainText("Error");
    await expect(fact(failing, "Unresolved")).toContainText("14 need attention");
    await expect(fact(failing, "Unresolved")).toContainText("3 rejected");
    await expect(fact(failing, "Next attempt")).toHaveText(/^(in \d+ s|now)$/);
    const destinationError = failing.getByRole("alert");
    await expect(destinationError).toContainText("connection refused (10.0.4.21:2575)");
    await expect(destinationError.getByText("Next step:")).toBeVisible();

    await expect(
      page.getByRole("table", { name: "Recent messages of this channel" }),
    ).toBeVisible();
    await expect(page.getByRole("table", { name: "Channel counters" })).toBeVisible();
    await expect(page.getByRole("list", { name: "Active alerts of this channel" })).toBeVisible();
    await expect(page.getByRole("table", { name: "Recent errors of this channel" })).toBeVisible();
    await expectTranscriptPanel(page, ["Source", "Destinations"]);
  });

  test("H-03: the last error names the failing destination, not the healthy source", async ({
    page,
  }) => {
    await openChannel(page, CHANNELS.sta);
    const note = lastErrorNote(page);
    await expect(note).toContainText("connection refused (10.0.4.21:2575)");
    // The endpoint of the error is the destination that failed.
    await expect(fact(note, "Endpoint")).toHaveText("Destination LIS HL7 out (backup)");
    await expect(fact(note, "Endpoint")).not.toContainText("STA Compact coag");
    // Its streak and delivery retry are the destination's own, apart from the source session.
    await expect(fact(note, "Failure streak")).toHaveText(/^[1-9]\d*$/);
    await expect(fact(note, "Delivery retry")).toHaveText(/^(in \d+ s|now)$/);
    await expect(fact(note, "Source session")).toContainText("Receiving (STA Compact coag)");
    await expect(fact(note, "Time")).toContainText(/\(.+ ago\)/);
    await expect(note.getByText("Next step:")).toBeVisible();
    await expect(note).toContainText("Check that the standby LIS interface is running");
  });

  test("H-03: a channel stopped for its configuration says why and offers no restart", async ({
    page,
  }) => {
    const channel = CHANNELS.outreach;
    await openChannel(page, channel);
    await expectIdentity(page, channel, /^Stopped: configuration invalid/);
    await expect(fact(main(page), "Stop reason")).toHaveText("The configuration is invalid");
    const note = lastErrorNote(page);
    await expect(note).toContainText("cannot listen on 10.0.4.5:2590: address already in use");
    await expect(fact(note, "Endpoint")).toHaveText("Source: Outreach HL7 listener");
    await expect(fact(note, "Retry")).toHaveText("No retry is scheduled");
    await expect(note).toContainText("Free port 2590");
    await expect(page.getByRole("button", { name: "Restart" })).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Resume" })).toHaveCount(0);
    await expect(page.getByRole("link", { name: "Edit" })).toBeVisible();
  });

  test("H-03: a paused channel reads as intentionally paused, with no error", async ({ page }) => {
    const channel = CHANNELS.vitros350;
    await openChannel(page, channel);
    await expectIdentity(page, channel, /^Paused by operator$/);
    await expect(main(page).getByText("Paused by operator").first()).toBeVisible();
    await expect(fact(main(page), "Stop reason")).toHaveText("Paused by an operator");
    await expect(main(page).getByText("No retry is scheduled: paused by operator")).toBeVisible();
    await expect(page.getByRole("button", { name: "Resume" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Pause" })).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Restart" })).toHaveCount(0);

    // No error styling or error block: nothing is announced as an alert or a last error.
    await expect(page.getByText("Last error", { exact: true })).toHaveCount(0);
    await expect(main(page).getByRole("alert")).toHaveCount(0);
    await expect(main(page).getByRole("status")).toHaveCount(0);
  });
});
