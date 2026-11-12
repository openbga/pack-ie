import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { chooseWithKeyboard } from "@/lib/test/keyboard";
import { renderWithProviders } from "@/lib/test/render";
import { chooseOption } from "@/lib/test/select";
import { page, rows, stubApi } from "./fixtures";
import { MessageFilterBar } from "./MessageFilterBar";
import { DEFAULT_FILTERS, type MessageFilters } from "./messageParams";

const CHANNELS = [
  { id: "ch-chem", label: "Synthetic chemistry" },
  { id: "ch-hema", label: "Synthetic hematology" },
];

/** The bar as a page holds it: the filters live in the caller, which hands each change back. */
function Harness({
  initial = DEFAULT_FILTERS,
  onChange,
}: {
  initial?: MessageFilters;
  onChange: (next: MessageFilters) => void;
}) {
  const [value, setValue] = useState(initial);
  return (
    <MessageFilterBar
      value={value}
      channels={CHANNELS}
      onChange={(next) => {
        setValue(next);
        onChange(next);
      }}
    />
  );
}

/** The counts the quick filters show come from one-row queries, whose totals this answers. */
const api = () =>
  stubApi({
    listMessages: async (query: { filter: { deliverySummaries?: string[]; states?: string[] } }) =>
      page(
        rows(1, 1),
        query.filter.deliverySummaries?.includes("attention")
          ? 12
          : query.filter.states?.includes("duplicate")
            ? 3
            : 0,
      ),
  });

const openMore = (user: ReturnType<typeof renderWithProviders>["user"]) =>
  user.click(screen.getByRole("button", { name: "More filters" }));

describe("MessageFilterBar", () => {
  it("reports channels, several at once, as the whole next filters", async () => {
    const onChange = vi.fn();
    const { user } = renderWithProviders(<Harness onChange={onChange} />, { api: api() });
    const trigger = screen.getByRole("button", { name: /Channels/ });
    expect(trigger.textContent).toContain("All");
    await user.click(trigger);
    const dialog = await screen.findByRole("dialog", { name: "Channels" });
    await user.click(within(dialog).getByRole("checkbox", { name: "Synthetic hematology" }));
    expect(onChange).toHaveBeenLastCalledWith({ ...DEFAULT_FILTERS, channels: ["ch-hema"] });
    await user.click(within(dialog).getByRole("checkbox", { name: "Synthetic chemistry" }));
    expect(onChange).toHaveBeenLastCalledWith({
      ...DEFAULT_FILTERS,
      channels: ["ch-hema", "ch-chem"],
    });
    await user.click(within(dialog).getByRole("button", { name: "Show all channels" }));
    expect(onChange).toHaveBeenLastCalledWith(DEFAULT_FILTERS);
  });

  it("says how many channels are selected", async () => {
    renderWithProviders(
      <Harness
        initial={{ ...DEFAULT_FILTERS, channels: ["ch-chem", "ch-hema"] }}
        onChange={vi.fn()}
      />,
      { api: api() },
    );
    expect(screen.getByRole("button", { name: /Channels/ }).textContent).toContain("2 selected");
  });

  it("names the one channel that is selected", async () => {
    renderWithProviders(
      <Harness initial={{ ...DEFAULT_FILTERS, channels: ["ch-hema"] }} onChange={vi.fn()} />,
      { api: api() },
    );
    expect(screen.getByRole("button", { name: /Channels/ }).textContent).toContain(
      "Synthetic hematology",
    );
  });

  it("keeps a channel that is no longer configured selectable, by its ID", () => {
    renderWithProviders(
      <Harness initial={{ ...DEFAULT_FILTERS, channels: ["ch-removed"] }} onChange={vi.fn()} />,
      { api: api() },
    );
    expect(screen.getByRole("button", { name: /Channels/ }).textContent).toContain("ch-removed");
  });

  it("puts direction, message state and delivery state under More filters, and says what is set", async () => {
    const onChange = vi.fn();
    const { user } = renderWithProviders(<Harness onChange={onChange} />, { api: api() });
    await openMore(user);
    await chooseOption(user, /Direction/, "Side A to side B");
    expect(onChange).toHaveBeenLastCalledWith({ ...DEFAULT_FILTERS, direction: "a_to_b" });
    await chooseOption(user, /Message state/, "Invalid");
    await chooseOption(user, /Delivery state/, "Failed, not delivered");
    expect(onChange).toHaveBeenLastCalledWith({
      ...DEFAULT_FILTERS,
      direction: "a_to_b",
      state: "invalid",
      delivery: "failed",
    });
    expect(
      screen.getByText(
        "Direction: Side A to side B · State: Invalid · Delivery: Failed, not delivered",
      ),
    ).toBeTruthy();
    await chooseOption(user, /Message state/, "Any message state");
    expect(onChange).toHaveBeenLastCalledWith({
      ...DEFAULT_FILTERS,
      direction: "a_to_b",
      delivery: "failed",
    });
  });

  it("has a quick filter for each of the four, with the count the engine gave", async () => {
    const onChange = vi.fn();
    const { user } = renderWithProviders(<Harness onChange={onChange} />, { api: api() });
    const group = screen.getByRole("group", { name: "Quick filters" });
    expect(
      within(group)
        .getAllByRole("button")
        .map((b) => b.textContent?.replace(/\s*\d*$/, "")),
    ).toEqual(["Needs attention", "Queued", "Duplicates", "Invalid or aborted"]);
    expect(await within(group).findByRole("button", { name: "Needs attention 12" })).toBeTruthy();
    expect(await within(group).findByRole("button", { name: "Duplicates 3" })).toBeTruthy();
    expect(within(group).getByRole("button", { name: "Queued 0" })).toBeTruthy();
    await user.click(within(group).getByRole("button", { name: "Needs attention 12" }));
    expect(onChange).toHaveBeenLastCalledWith({ ...DEFAULT_FILTERS, quick: "attention" });
    expect(
      within(group)
        .getByRole("button", { name: "Needs attention 12" })
        .getAttribute("aria-pressed"),
    ).toBe("true");
    await user.click(within(group).getByRole("button", { name: "Duplicates 3" }));
    expect(onChange).toHaveBeenLastCalledWith({ ...DEFAULT_FILTERS, quick: "duplicates" });
    await user.click(within(group).getByRole("button", { name: "Duplicates 3" }));
    expect(onChange).toHaveBeenLastCalledWith(DEFAULT_FILTERS);
  });

  it("counts a quick filter within the other filters, with one row asked for", async () => {
    const stub = api();
    renderWithProviders(
      <Harness initial={{ ...DEFAULT_FILTERS, channels: ["ch-chem"] }} onChange={vi.fn()} />,
      { api: stub },
    );
    await waitFor(() => expect(stub.listMessages).toHaveBeenCalledTimes(4));
    for (const [query] of stub.listMessages.mock.calls) {
      expect(query).toMatchObject({ offset: 0, limit: 1, filter: { channelIds: ["ch-chem"] } });
    }
  });

  it("lets a quick filter replace the state choices, and a state choice replace it", async () => {
    const onChange = vi.fn();
    const { user } = renderWithProviders(
      <Harness
        initial={{ ...DEFAULT_FILTERS, state: "invalid", delivery: "failed" }}
        onChange={onChange}
      />,
      { api: api() },
    );
    await user.click(await screen.findByRole("button", { name: /^Queued/ }));
    expect(onChange).toHaveBeenLastCalledWith({ ...DEFAULT_FILTERS, quick: "queued" });
    await openMore(user);
    await chooseOption(user, /Message state/, "Filtered");
    expect(onChange).toHaveBeenLastCalledWith({ ...DEFAULT_FILTERS, state: "filtered" });
  });

  it("sets the time range from the quick ranges, and opens it with Any time", async () => {
    const onChange = vi.fn();
    const { user } = renderWithProviders(<Harness onChange={onChange} />, { api: api() });
    expect(screen.getByRole("button", { name: /Time range/ }).textContent).toContain(
      "Last 24 hours",
    );
    await chooseOption(user, /Time range/, "Last hour");
    expect(onChange).toHaveBeenLastCalledWith({ channels: [], from: "-1h" });
    await chooseOption(user, /Time range/, "Last 7 days");
    expect(onChange).toHaveBeenLastCalledWith({ channels: [], from: "-7d" });
    await chooseOption(user, /Time range/, "Last 30 days");
    expect(onChange).toHaveBeenLastCalledWith({ channels: [], from: "-30d" });
    await chooseOption(user, /Time range/, "Any time");
    expect(onChange).toHaveBeenLastCalledWith({ channels: [] });
    expect(screen.queryByLabelText("From")).toBeNull();
  });

  it("takes custom times, reporting once a time is entered and leaving half-typed ones alone", async () => {
    const onChange = vi.fn();
    const { user } = renderWithProviders(
      <Harness initial={{ channels: [] }} onChange={onChange} />,
      { api: api() },
    );
    await chooseOption(user, /Time range/, "Custom range…");
    expect(onChange).not.toHaveBeenCalled();
    const from = screen.getByLabelText("From");
    const to = screen.getByLabelText("To");
    fireEvent.change(from, { target: { value: "" } });
    fireEvent.blur(from);
    expect(onChange).not.toHaveBeenCalled();
    fireEvent.change(from, { target: { value: "2026-11-01T08:00" } });
    fireEvent.blur(from);
    expect(onChange).toHaveBeenLastCalledWith({
      channels: [],
      from: new Date("2026-11-01T08:00").toISOString(),
    });
    fireEvent.change(to, { target: { value: "2026-11-02T18:30" } });
    fireEvent.keyDown(to, { key: "Enter" });
    expect(onChange).toHaveBeenLastCalledWith({
      channels: [],
      from: new Date("2026-11-01T08:00").toISOString(),
      to: new Date("2026-11-02T18:30").toISOString(),
    });
    expect(screen.getByRole("button", { name: /Time range/ }).textContent).toContain(
      "Custom range",
    );
  });

  it("writes a quick range out as times when Custom is chosen, so they can be edited", async () => {
    const onChange = vi.fn();
    const { user } = renderWithProviders(<Harness onChange={onChange} />, { api: api() });
    const before = Date.now();
    await chooseOption(user, /Time range/, "Custom range…");
    const [[next]] = onChange.mock.calls as [[MessageFilters]];
    const from = new Date(next.from ?? "").getTime();
    expect(from).toBeGreaterThanOrEqual(before - 86_400_000);
    expect(from).toBeLessThanOrEqual(Date.now() - 86_400_000 + 1);
    expect((screen.getByLabelText("From") as HTMLInputElement).value).not.toBe("");
    expect(next.to).toBeUndefined();
  });

  it("finds by identifier and by text after a pause in typing, not on every key", async () => {
    const onChange = vi.fn();
    const { user } = renderWithProviders(<Harness onChange={onChange} />, { api: api() });
    await user.type(screen.getByRole("searchbox", { name: "Identifier" }), "CTRL-000042");
    expect(onChange).not.toHaveBeenCalled();
    await waitFor(() => expect(onChange).toHaveBeenCalledTimes(1));
    expect(onChange).toHaveBeenLastCalledWith({ ...DEFAULT_FILTERS, id: "CTRL-000042" });
    await user.type(screen.getByRole("searchbox", { name: "Message text" }), "glucose{Enter}");
    expect(onChange).toHaveBeenLastCalledWith({
      ...DEFAULT_FILTERS,
      id: "CTRL-000042",
      text: "glucose",
    });
  });

  it("does not lose the first find when the second is typed within the pause", async () => {
    const onChange = vi.fn();
    const { user } = renderWithProviders(<Harness onChange={onChange} />, { api: api() });
    await user.type(screen.getByRole("searchbox", { name: "Identifier" }), "A1");
    await user.type(screen.getByRole("searchbox", { name: "Message text" }), "b2");
    await waitFor(() =>
      expect(onChange).toHaveBeenLastCalledWith({ ...DEFAULT_FILTERS, id: "A1", text: "b2" }),
    );
  });

  it("asks for an exact identifier match in More filters", async () => {
    const onChange = vi.fn();
    const { user } = renderWithProviders(<Harness onChange={onChange} />, { api: api() });
    await openMore(user);
    expect(screen.getByRole("button", { name: /Identifier match/ }).textContent).toContain(
      "Starts with",
    );
    await chooseOption(user, /Identifier match/, "Exactly");
    expect(onChange).toHaveBeenLastCalledWith({ ...DEFAULT_FILTERS, idExact: true });
    expect(screen.getByText("Identifier: exactly")).toBeTruthy();
    await chooseOption(user, /Identifier match/, "Starts with");
    expect(onChange).toHaveBeenLastCalledWith(DEFAULT_FILTERS);
  });

  it("looks a message up by the start of its ID in More filters, and says what it needs", async () => {
    const onChange = vi.fn();
    const { user } = renderWithProviders(<Harness onChange={onChange} />, { api: api() });
    await openMore(user);
    await user.type(screen.getByRole("searchbox", { name: "Message ID" }), "0a1b{Enter}");
    expect(onChange).toHaveBeenLastCalledWith({ ...DEFAULT_FILTERS, messageId: "0a1b" });
    expect(
      screen.getByText("A message ID needs 8 to 32 hex digits, dashes optional."),
    ).toBeTruthy();
    expect(screen.getByText("Message ID: starts with 0a1b")).toBeTruthy();
    await user.type(screen.getByRole("searchbox", { name: "Message ID" }), "2c3d{Enter}");
    expect(onChange).toHaveBeenLastCalledWith({ ...DEFAULT_FILTERS, messageId: "0a1b2c3d" });
    expect(
      screen.queryByText("A message ID needs 8 to 32 hex digits, dashes optional."),
    ).toBeNull();
  });

  it("labels the two searches Identifier and Message text, so the labels tell them apart", () => {
    renderWithProviders(
      <Harness
        initial={{ ...DEFAULT_FILTERS, id: "PAT-TEST-1", text: "glucose" }}
        onChange={vi.fn()}
      />,
      { api: api() },
    );
    // The visible label is the criterion's name; the accessible name is the same.
    expect(screen.getByText("Identifier", { selector: "label [aria-hidden]" })).toBeTruthy();
    expect(screen.getByText("Message text", { selector: "label [aria-hidden]" })).toBeTruthy();
    expect(screen.queryByText("Find")).toBeNull();
    expect((screen.getByRole("searchbox", { name: "Identifier" }) as HTMLInputElement).value).toBe(
      "PAT-TEST-1",
    );
    expect(
      (screen.getByRole("searchbox", { name: "Message text" }) as HTMLInputElement).value,
    ).toBe("glucose");
  });

  it("says a text of fewer than 3 characters is not searched", () => {
    renderWithProviders(
      <Harness initial={{ ...DEFAULT_FILTERS, text: "ab" }} onChange={vi.fn()} />,
      { api: api() },
    );
    expect(screen.getByText("Type at least 3 characters to search text.")).toBeTruthy();
    expect(screen.queryByText("Text search covers the selected time range only.")).toBeNull();
  });

  it("notes that a text search covers the selected time range only", () => {
    renderWithProviders(
      <Harness initial={{ ...DEFAULT_FILTERS, text: "glucose" }} onChange={vi.fn()} />,
      { api: api() },
    );
    expect(screen.getByText("Text search covers the selected time range only.")).toBeTruthy();
  });

  it("shows Clear filters only while a filter differs from the defaults, and clears them all", async () => {
    const onChange = vi.fn();
    const { user, unmount } = renderWithProviders(<Harness onChange={onChange} />, { api: api() });
    expect(screen.queryByRole("button", { name: "Clear filters" })).toBeNull();
    unmount();
    renderWithProviders(
      <Harness
        initial={{ channels: ["ch-chem"], id: "X", from: "-7d", quick: "attention" }}
        onChange={onChange}
      />,
      { api: api() },
    );
    await user.click(screen.getByRole("button", { name: "Clear filters" }));
    expect(onChange).toHaveBeenLastCalledWith(DEFAULT_FILTERS);
    expect(screen.queryByRole("button", { name: "Clear filters" })).toBeNull();
    expect((screen.getByRole("searchbox", { name: "Identifier" }) as HTMLInputElement).value).toBe(
      "",
    );
    expect(screen.getByRole("button", { name: /Time range/ }).textContent).toContain(
      "Last 24 hours",
    );
  });

  it("offers Clear filters for a range other than the default", () => {
    renderWithProviders(<Harness initial={{ channels: [] }} onChange={vi.fn()} />, { api: api() });
    expect(screen.getByRole("button", { name: "Clear filters" })).toBeTruthy();
  });

  it("is operable with the keyboard alone", async () => {
    const onChange = vi.fn();
    const { user } = renderWithProviders(<Harness onChange={onChange} />, { api: api() });
    await chooseWithKeyboard(user, /Time range/, "Last 7 days");
    expect(onChange).toHaveBeenLastCalledWith({ channels: [], from: "-7d" });
  });

  it("has no accessibility violations, with times and filters showing", async () => {
    const { container, user } = renderWithProviders(
      <Harness
        initial={{
          channels: ["ch-chem"],
          from: "2026-11-01T08:00:00Z",
          quick: "attention",
          text: "ab",
        }}
        onChange={vi.fn()}
      />,
      { api: api() },
    );
    await screen.findByRole("button", { name: "Needs attention 12" });
    await openMore(user);
    await expectNoA11yViolations(container);
  });
});
