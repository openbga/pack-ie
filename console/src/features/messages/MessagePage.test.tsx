import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { engineError } from "@/lib/test/engineError";
import { renderRoutes } from "@/lib/test/router";
import { delivery, detail, snapshot, stubApi, summary } from "./fixtures";
import { findRecord, nonRouted, PATH, ROUTES } from "./messageRecordSetup";

describe("MessagePage", () => {
  it("shows the record: identifier, state, facts, and no violations", async () => {
    const { container } = await findRecord(
      stubApi({
        getMessage: async () =>
          detail({
            sourceFile: { name: "results.csv", fileHash: "f".repeat(64), firstRow: 2, lastRow: 5 },
          }),
      }),
    );
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("CTRL-000001");
    expect(screen.getByText("Routed")).toBeTruthy();
    const crumbs = within(screen.getByRole("navigation", { name: "Breadcrumb" }));
    expect(crumbs.getByRole("link", { name: "Messages" }).getAttribute("href")).toBe("/messages");
    const facts = within(container.querySelector("dl") as HTMLElement);
    expect(facts.getByRole("link", { name: "Synthetic chemistry" }).getAttribute("href")).toBe(
      "/channels/ch-chem",
    );
    for (const text of [
      "CTRL-000001",
      "SMP-000001",
      "PAT-TEST-000001",
      "ORU^R01",
      "HL7 v2",
      "Inbound",
      "Received from the sender",
      "1.5 KB",
      "msg-000001",
      "Synthetic analyzer",
      "10.0.0.5:4000",
      "results.csv, rows 2 to 5",
      "7",
    ]) {
      expect(facts.getByText(text), text).toBeTruthy();
    }
    expect(facts.getByText("Response sent to the sender").nextElementSibling?.textContent).toMatch(
      /^AA at /,
    );
    // The record carries no parsed fields: they are read on request only.
    expect(screen.queryByText(/parsed/i)).toBeNull();
    await expectNoA11yViolations(container);
  });

  it("shows the reason a delivery was resent with, and nothing for one that was not", async () => {
    await findRecord(
      stubApi({
        getMessage: async () =>
          detail({
            deliveries: [
              delivery("LIS", { resendReason: "Destination was down" }),
              delivery("Archive"),
            ],
          }),
      }),
    );
    const lis = within(screen.getByRole("article", { name: "LIS" }));
    expect(lis.getByText("Resent").nextElementSibling?.textContent).toBe("Destination was down");
    const archive = within(screen.getByRole("article", { name: "Archive" }));
    expect(archive.queryByText("Resent")).toBeNull();
  });

  it("shows the last error of a failed delivery, and nothing for one without", async () => {
    await findRecord(
      stubApi({
        getMessage: async () =>
          detail({
            deliveries: [
              delivery("LIS", { lastError: "connection refused by 10.0.0.9:5402" }),
              delivery("Archive"),
            ],
          }),
      }),
    );
    const lis = within(screen.getByRole("article", { name: "LIS" }));
    expect(lis.getByText("Last error").nextElementSibling?.textContent).toBe(
      "connection refused by 10.0.0.9:5402",
    );
    const archive = within(screen.getByRole("article", { name: "Archive" }));
    expect(archive.queryByText("Last error")).toBeNull();
  });

  it("falls back to the sample id, then the message id, as the identifier", async () => {
    await findRecord(
      stubApi({
        getMessage: async () => detail({ summary: summary(1, { controlId: null }) }),
      }),
    );
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("SMP-000001");
  });

  it("says what the protocol did not provide, and that nothing was sent to the sender", async () => {
    await findRecord(
      stubApi({
        getMessage: async () =>
          detail({
            summary: summary(1, {
              controlId: null,
              sampleId: null,
              patientId: null,
              messageType: null,
            }),
            responseCode: null,
            respondedAt: null,
            committedAt: null,
          }),
      }),
    );
    expect(screen.getAllByText("Not provided")).toHaveLength(4);
    expect(screen.getByText("None sent")).toBeTruthy();
    expect(screen.getByText("Not complete yet")).toBeTruthy();
  });

  describe("messages that were not routed", () => {
    it.each([
      [
        "invalid",
        "Invalid message",
        /could not be parsed or validated/,
        "parse_error",
        /could not be parsed\./,
      ],
      ["aborted", "Aborted message", /ended before a complete message arrived/, null, null],
      [
        "filtered",
        "Filtered message",
        /Nothing was sent/,
        "source_filter",
        /filter of the channel excluded it/,
      ],
      ["duplicate", "Duplicate message", /repeats an earlier message/, null, null],
      ["receiving", "Receiving message", /transfer is in progress/, null, null],
      ["received", "Received message", /has not finished processing/, null, null],
      ["observed", "Observed message", /never processed/, null, null],
    ] as const)(
      "explains a %s message, with its reason in words",
      async (state, title, meaning, code, words) => {
        await findRecord(
          stubApi({
            getMessage: async () =>
              nonRouted(state, {
                reasonCode: code,
                abortReason: state === "aborted" ? "receive_timeout" : null,
              }),
          }),
        );
        const notice = screen
          .getAllByRole(state === "invalid" || state === "aborted" ? "alert" : "status")
          .find((element) => element.textContent?.includes(title));
        expect(notice, title).toBeTruthy();
        expect(within(notice as HTMLElement).getByText(meaning)).toBeTruthy();
        if (words) expect(within(notice as HTMLElement).getByText(words)).toBeTruthy();
        if (state === "aborted") {
          expect(
            within(notice as HTMLElement).getByText(/No data arrived within the receive timeout/),
          ).toBeTruthy();
        }
        expect(
          screen.getByText(
            "This message has no deliveries: nothing was queued for any destination.",
          ),
        ).toBeTruthy();
      },
    );

    it("links a duplicate to its original, a resend to its source and a revision to its earlier message", async () => {
      await findRecord(
        stubApi({
          getMessage: async () =>
            nonRouted("duplicate", {
              duplicateOf: "msg-000000",
              revisionOf: "msg-revised",
              summary: summary(1, {
                state: "duplicate",
                origin: "resend",
                resentFrom: "msg-source",
                deliveries: [],
                deliverySummary: "none",
              }),
            }),
        }),
      );
      const link = (name: string) => screen.getByRole("link", { name }).getAttribute("href");
      expect(link("The original message")).toBe("/messages/msg-000000");
      expect(link("The first message")).toBe("/messages/msg-source");
      expect(link("The earlier message")).toBe("/messages/msg-revised");
    });

    it("offers a resend only for the messages the channel processed or tried to", async () => {
      for (const [state, offered] of [
        ["invalid", true],
        ["aborted", true],
        ["filtered", true],
        ["duplicate", true],
        ["receiving", false],
        ["received", false],
        ["observed", false],
      ] as const) {
        const { unmount } = await findRecord(stubApi({ getMessage: async () => nonRouted(state) }));
        expect(screen.queryByRole("button", { name: "Resend…" }) !== null, state).toBe(offered);
        unmount();
      }
    });
  });

  it("explains a message nobody can find, without an error alert", async () => {
    const api = stubApi({
      getMessage: async () => {
        throw engineError("not_found", "Gone.");
      },
    });
    const { user } = renderRoutes(ROUTES, PATH, { api });
    expect(await screen.findByText("No stored message has this ID")).toBeTruthy();
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Message not found");
    expect(screen.queryByRole("alert")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Back to messages" }));
    expect(await screen.findByText(/Navigated to \/messages/)).toBeTruthy();
  });

  it("says the store is not available when the engine does not serve it, and not that it failed", async () => {
    const api = stubApi({
      getMessage: async () => {
        throw engineError("not_implemented", "No store.");
      },
    });
    renderRoutes(ROUTES, PATH, { api });
    expect(await screen.findByText("The message store is not available")).toBeTruthy();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Message");
  });

  it("says the store is not available when the snapshot reports it, with the reason", async () => {
    const api = stubApi({
      getSnapshot: async () =>
        snapshot({
          state: "unavailable",
          messageCount: null,
          databaseBytes: null,
          diskFreeBytes: null,
          diskTotalBytes: null,
        }),
      getMessage: async () => {
        throw engineError("unavailable", "The store is down.");
      },
    });
    renderRoutes(ROUTES, PATH, { api });
    expect(await screen.findByText("The message store is not available")).toBeTruthy();
    expect(screen.getByText(/reports that its message store is unavailable/)).toBeTruthy();
  });

  it("shows another failure with a retry", async () => {
    let fail = true;
    const api = stubApi({
      getMessage: async () => {
        if (fail) throw engineError("internal", "The store is busy.");
        return detail();
      },
    });
    const { user } = renderRoutes(ROUTES, PATH, { api });
    expect(await screen.findByText("The message could not be loaded")).toBeTruthy();
    expect(screen.getByText("The store is busy.")).toBeTruthy();
    fail = false;
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByRole("heading", { level: 2, name: "Deliveries" })).toBeTruthy();
  });

  it("reads the record again while a delivery is in progress, and stops when none is", async () => {
    const api = stubApi({
      getMessage: async () =>
        detail({ deliveries: [delivery("LIS", { state: "sending", ackLevel: "none" })] }),
    });
    await findRecord(api);
    await waitFor(() => expect(api.getMessage.mock.calls.length).toBeGreaterThan(1), {
      timeout: 5000,
    });
  });
});
