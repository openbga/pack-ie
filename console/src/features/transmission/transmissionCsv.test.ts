import { describe, expect, it } from "vitest";
import { report, row } from "./fixtures";
import {
  TRANSMISSION_CSV_HEADER,
  TRUNCATED_MARKER,
  transmissionCsv,
  transmissionFile,
  transmissionFileName,
} from "./transmissionCsv";

const lines = (csv: string) => csv.split("\r\n").slice(0, -1);

describe("transmissionCsv", () => {
  it("has a header row and one CRLF-ended line per row", () => {
    const made = report();
    const csv = transmissionCsv(made, made.rows);
    expect(csv.endsWith("\r\n")).toBe(true);
    expect(lines(csv)).toHaveLength(1 + made.rows.length);
    expect(lines(csv)[0]).toBe(TRANSMISSION_CSV_HEADER.join(","));
    expect(transmissionCsv(made, [])).toBe(`${TRANSMISSION_CSV_HEADER.join(",")}\r\n`);
  });

  it("makes each line say what it is: channel, period and time of the report first", () => {
    const made = report();
    const [first] = lines(transmissionCsv(made, made.rows)).slice(1);
    expect(
      first?.startsWith(
        "ch-msg-sta,2026-11-06T00:00:00.000Z,2026-11-07T00:00:00.000Z,2026-11-07T08:30:00.000Z,msg-1,",
      ),
    ).toBe(true);
  });

  it("holds every field of the row, both hashes and the acknowledgement code included", () => {
    const made = report();
    const [, first, second] = lines(transmissionCsv(made, made.rows));
    expect(first?.split(",")).toEqual([
      "ch-msg-sta",
      "2026-11-06T00:00:00.000Z",
      "2026-11-07T00:00:00.000Z",
      "2026-11-07T08:30:00.000Z",
      "msg-1",
      "2026-11-06T08:01:00.000Z",
      "ep-analyzer",
      "10.0.4.17:41234",
      "12",
      "12",
      "routed",
      "CTL-1",
      "a".repeat(64),
      "dest-lis",
      "LIS (HL7)",
      "acknowledged",
      "accepted",
      "AA",
      "ACK-1",
      "b".repeat(64),
      "1",
      "2026-11-06T08:01:01.000Z",
      "2026-11-06T08:01:02.000Z",
    ]);
    // A missing value is an empty cell; the destination's label and the engine's codes are kept.
    expect(second?.split(",").slice(13)).toEqual([
      "dest-archive",
      "Archive file",
      "acknowledged",
      "received",
      "CA",
      "ACK-1",
      "",
      "1",
      "2026-11-06T08:01:01.000Z",
      "2026-11-06T08:01:02.000Z",
    ]);
  });

  it("writes only the rows given, so a narrowed grid exports what it shows", () => {
    const made = report();
    const archive = made.rows.filter((each) => each.destinationId === "dest-archive");
    expect(lines(transmissionCsv(made, archive))).toHaveLength(2);
  });

  it("ends a truncated report with the engine's marker line, narrowed or not", () => {
    const made = report({ truncated: true, periodRowsTotal: 51_651, messages: 12_000 });
    const whole = lines(transmissionCsv(made, made.rows));
    expect(whole.at(-1)).toBe(TRUNCATED_MARKER);
    // Before it, how much of the period the file holds: the rows in the report, the period's
    // deliveries, the limit and the messages received.
    expect(whole.at(-2)).toBe(
      "# listed: 5 of 51651 deliveries of the period were in the report (limit 10000); 12000 messages were received",
    );
    expect(whole).toHaveLength(3 + made.rows.length);
    const archive = made.rows.filter((each) => each.destinationId === "dest-archive");
    expect(lines(transmissionCsv(made, archive)).at(-1)).toBe(TRUNCATED_MARKER);
    // The text is the one `packie messages verify --csv` ends with.
    expect(TRUNCATED_MARKER).toBe(
      "# truncated: more than 10000 deliveries in the period; the rows above are the first 10000. Report a shorter period to list every delivery.",
    );
  });

  it("has no marker for a complete report", () => {
    const made = report();
    expect(transmissionCsv(made, made.rows)).not.toContain("# truncated");
  });

  it("guards text a spreadsheet would read as a formula", () => {
    const made = report({ rows: [row(1, { controlId: "=HYPERLINK(1)" })] });
    expect(lines(transmissionCsv(made, made.rows))[1]).toContain(",'=HYPERLINK(1),");
  });
});

describe("transmissionFileName", () => {
  it("is packie-transmission-<channel>-<date>.csv, the date being the day of the report", () => {
    expect(transmissionFileName(report())).toBe("packie-transmission-ch-msg-sta-2026-11-07.csv");
    expect(transmissionFileName(report({ channelId: "ch/odd id" }))).toBe(
      "packie-transmission-ch_odd_id-2026-11-07.csv",
    );
  });

  it("is saved as a CSV report file", () => {
    const made = report();
    expect(transmissionFile(made, made.rows)).toMatchObject({
      name: "packie-transmission-ch-msg-sta-2026-11-07.csv",
      format: "csv",
    });
  });
});
