import { screen, within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { mockGridLayout } from "@/lib/test/layout";
import { renderRoutes } from "@/lib/test/router";
import { chip, page, stubApi, summary } from "./fixtures";
import { MessagesPage } from "./MessagesPage";
import { forgetMessagesSession } from "./messagesSession";

const ROUTES = [{ path: "/messages", element: <MessagesPage /> }];

/** jsdom has no layout: every element is `width` pixels wide. */
function widthOf(width: number) {
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
    width,
    height: 32,
    x: 0,
    y: 0,
    top: 0,
    left: 0,
    right: width,
    bottom: 32,
    toJSON: () => ({}),
  });
}

const headers = () =>
  within(screen.getByRole("grid"))
    .getAllByRole("columnheader")
    .map((header) => header.textContent);

/** Opens the column chooser, takes the default set and closes it again. */
async function resetColumns(user: UserEvent) {
  await user.click(screen.getByRole("button", { name: /Columns/ }));
  await user.click(
    within(await screen.findByRole("dialog", { name: "Columns" })).getByRole("button", {
      name: "Default columns",
    }),
  );
  await user.keyboard("{Escape}");
}

const FIRST_FIVE = ["Received", "Channel", "Message state", "Delivery summary", "Identifier"];

async function list(width: number) {
  widthOf(width);
  const api = stubApi({
    listMessages: async () =>
      page([
        summary(1, {
          deliveries: [
            chip("Billing export (SFTP) with a long name", { state: "sending", ackLevel: "none" }),
            chip("Results archive folder", { state: "failed", ackLevel: "none" }),
            chip("Third destination", { state: "queued", ackLevel: "none" }),
            chip("Fourth destination", { state: "acknowledged", ackLevel: "received" }),
          ],
        }),
      ]),
  });
  const rendered = renderRoutes(ROUTES, "/messages", { api });
  await screen.findByRole("grid");
  return rendered;
}

describe("the Messages grid's columns", () => {
  beforeEach(() => {
    mockGridLayout();
    forgetMessagesSession();
  });

  it("shows every default column where they all fit", async () => {
    await list(1700);
    expect(headers()).toEqual([
      "Received",
      "Channel",
      "Direction",
      "Message state",
      "Delivery summary",
      "Message type",
      "Identifier",
      "Patient ID",
      "Size",
      "Actions",
    ]);
    expect(screen.getByRole("button", { name: "Columns: Default" })).toBeTruthy();
  });

  it("drops columns from the right as the grid narrows, never the five that stay", async () => {
    const { unmount } = await list(1000);
    expect(headers()).toEqual([
      "Received",
      "Channel",
      "Direction",
      "Message state",
      "Delivery summary",
      "Message type",
      "Identifier",
      "Actions",
    ]);
    expect(screen.getByRole("button", { name: "Columns: Default (2 hidden to fit)" })).toBeTruthy();
    unmount();
    vi.restoreAllMocks();
    mockGridLayout();
    await list(900);
    expect(headers()).toEqual([
      "Received",
      "Channel",
      "Direction",
      "Message state",
      "Delivery summary",
      "Identifier",
      "Actions",
    ]);
  });

  it.each([760, 640, 300])(
    "keeps Received, Channel, state, deliveries and Identifier at %i",
    async (width) => {
      await list(width);
      expect(headers()).toEqual([...FIRST_FIVE, "Actions"]);
      expect(
        screen.getByRole("button", { name: "Columns: Default (4 hidden to fit)" }),
      ).toBeTruthy();
    },
  );

  it("brings a dropped column back through the chooser, and the default set returns", async () => {
    const { user } = await list(760);
    await user.click(screen.getByRole("button", { name: /Columns/ }));
    const dialog = await screen.findByRole("dialog", { name: "Columns" });
    // The five that stay are not offered; the dropped default columns are, unchecked.
    expect(
      within(dialog)
        .getAllByRole("checkbox")
        .map((box) => [box.closest("label")?.textContent, (box as HTMLInputElement).checked]),
    ).toEqual([
      ["Direction", false],
      ["Message type", false],
      ["Patient ID", false],
      ["Size", false],
      ["Message ID", false],
      ["Source peer", false],
      ["Configuration version", false],
      ["Resent from", false],
    ]);
    await user.click(within(dialog).getByRole("checkbox", { name: "Patient ID" }));
    await user.keyboard("{Escape}");
    expect(headers()).toEqual([...FIRST_FIVE.slice(0, 4), "Identifier", "Patient ID", "Actions"]);
    expect(screen.getByRole("button", { name: "Columns: Custom" })).toBeTruthy();
    await resetColumns(user);
    expect(headers()).toEqual([...FIRST_FIVE, "Actions"]);
  });

  it("lets a column that fits be taken away, and keeps the choice on a wide grid", async () => {
    const { user } = await list(1700);
    await user.click(screen.getByRole("button", { name: /Columns/ }));
    const dialog = await screen.findByRole("dialog", { name: "Columns" });
    await user.click(within(dialog).getByRole("checkbox", { name: "Size" }));
    await user.keyboard("{Escape}");
    expect(headers()).not.toContain("Size");
    expect(headers()).toContain("Patient ID");
  });

  it("shows the time compactly with the full time as its title", async () => {
    await list(1700);
    const cell = within(screen.getAllByRole("row")[1] as HTMLElement).getAllByRole("gridcell")[0];
    const time = cell?.querySelector("time");
    expect(time?.textContent).toMatch(/^\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/);
    expect(time?.getAttribute("title")).toMatch(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}/);
  });

  it("shows the direction as an arrow and a few words, the full wording in its title", async () => {
    widthOf(1700);
    const api = stubApi({
      listMessages: async () =>
        page([
          summary(1, { direction: "inbound" }),
          summary(2, { direction: "a_to_b" }),
          summary(3, { direction: "b_to_a" }),
        ]),
    });
    renderRoutes(ROUTES, "/messages", { api });
    await screen.findByRole("grid");
    for (const [text, title] of [
      ["Inbound", "Inbound"],
      ["A to B", "Side A to side B"],
      ["B to A", "Side B to side A"],
    ] as const) {
      expect(
        within(screen.getByRole("grid")).getByText(text).closest("[title]")?.getAttribute("title"),
      ).toBe(title);
    }
  });

  it("shows one chip per line, two at most, and counts the rest in a title", async () => {
    await list(1700);
    const chips = within(screen.getByRole("list", { name: "Deliveries" })).getAllByRole("listitem");
    expect(chips.map((item) => item.textContent)).toEqual([
      "SendingBilling export (SFTP) with a long name",
      "FailedResults archive folder",
      "+2 more",
    ]);
    expect(chips[0]?.getAttribute("title")).toBe(
      "Billing export (SFTP) with a long name: Sending. No acknowledgement was received.",
    );
    expect(chips[2]?.getAttribute("title")).toBe(
      "Third destination: Queued\nFourth destination: Acknowledged (received)",
    );
  });

  it("says what an acknowledgement proved in the chip, and never just 'delivered'", async () => {
    widthOf(1700);
    const api = stubApi({
      listMessages: async () =>
        page([
          summary(1, { deliveries: [chip("A", { ackLevel: "handed_off" })] }),
          summary(2, { deliveries: [chip("B", { ackLevel: "received" })] }),
          summary(3, { deliveries: [chip("C", { ackLevel: "accepted" })] }),
        ]),
    });
    renderRoutes(ROUTES, "/messages", { api });
    await screen.findByRole("grid");
    const chips = within(screen.getByRole("grid")).getAllByRole("listitem");
    expect(chips.map((item) => item.textContent)).toEqual(["WrittenA", "ReceivedB", "AcceptedC"]);
    expect(chips.map((item) => item.getAttribute("title"))).toEqual([
      "A: Acknowledged (written). Written to the destination. Not confirmed by it.",
      "B: Acknowledged (received). Received by the destination. Processing not confirmed.",
      "C: Acknowledged (accepted). Accepted by the destination's application.",
    ]);
  });

  it("adds optional columns through the column chooser, and takes them away again", async () => {
    const api = stubApi({
      listMessages: async () => page([summary(1, { origin: "resend", resentFrom: "msg-000000" })]),
    });
    const { user } = renderRoutes(ROUTES, "/messages", { api });
    await screen.findByRole("grid");
    await user.click(screen.getByRole("button", { name: /Columns/ }));
    const dialog = await screen.findByRole("dialog", { name: "Columns" });
    for (const name of ["Message ID", "Source peer", "Configuration version", "Resent from"]) {
      await user.click(within(dialog).getByRole("checkbox", { name }));
    }
    await user.keyboard("{Escape}");
    for (const name of ["Message ID", "Source peer", "Configuration version", "Resent from"]) {
      expect(screen.getByRole("columnheader", { name })).toBeTruthy();
    }
    expect(screen.getByRole("gridcell", { name: "10.0.0.5:4000" })).toBeTruthy();
    expect(screen.getByRole("gridcell", { name: "msg-000000" })).toBeTruthy();
    await resetColumns(user);
    expect(screen.queryByRole("columnheader", { name: "Source peer" })).toBeNull();
  });
});
