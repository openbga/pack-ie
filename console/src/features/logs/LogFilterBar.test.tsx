import { fireEvent, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { chooseWithKeyboard } from "@/lib/test/keyboard";
import { renderWithProviders } from "@/lib/test/render";
import { chooseOption } from "@/lib/test/select";
import { LogFilterBar } from "./LogFilterBar";
import { DEFAULT_FILTERS, type LogFilters } from "./logParams";

const CHANNELS = [
  { id: "ch-chem", label: "Synthetic chemistry" },
  { id: "ch-hema", label: "Synthetic hematology" },
];
const ENDPOINTS = [{ id: "ep-lis", label: "Synthetic LIS port" }];

function Harness({
  initial = DEFAULT_FILTERS,
  onChange,
}: {
  initial?: LogFilters;
  onChange: (next: LogFilters) => void;
}) {
  const [value, setValue] = useState(initial);
  return (
    <LogFilterBar
      value={value}
      channels={CHANNELS}
      endpoints={ENDPOINTS}
      count="3 loaded"
      onChange={(next) => {
        setValue(next);
        onChange(next);
      }}
    />
  );
}

describe("LogFilterBar", () => {
  it("starts at Info and above, live, with nothing to clear", () => {
    renderWithProviders(<Harness onChange={vi.fn()} />);
    expect(screen.getByRole("button", { name: /Level/ }).textContent).toContain("Info and above");
    expect(screen.getByRole("button", { name: /Time range/ }).textContent).toContain("Live");
    expect(screen.queryByRole("button", { name: "Clear filters" })).toBeNull();
    expect(screen.getByText("3 loaded")).toBeTruthy();
  });

  it("reports the minimum level, from error only to everything", async () => {
    const onChange = vi.fn();
    const { user } = renderWithProviders(<Harness onChange={onChange} />);
    await chooseOption(user, /Level/, "Warning and above");
    expect(onChange).toHaveBeenLastCalledWith({ ...DEFAULT_FILTERS, level: "warn" });
    await chooseOption(user, /Level/, "Error only");
    expect(onChange).toHaveBeenLastCalledWith({ ...DEFAULT_FILTERS, level: "error" });
    await chooseOption(user, /Level/, "Everything (trace)");
    expect(onChange).toHaveBeenLastCalledWith({ ...DEFAULT_FILTERS, level: "trace" });
  });

  it("reports a channel and an endpoint, and clears each with All", async () => {
    const onChange = vi.fn();
    const { user } = renderWithProviders(<Harness onChange={onChange} />);
    await chooseOption(user, /Channel/, "Synthetic hematology");
    expect(onChange).toHaveBeenLastCalledWith({ ...DEFAULT_FILTERS, channel: "ch-hema" });
    await chooseOption(user, /Endpoint/, "Synthetic LIS port");
    expect(onChange).toHaveBeenLastCalledWith({
      ...DEFAULT_FILTERS,
      channel: "ch-hema",
      endpoint: "ep-lis",
    });
    await chooseOption(user, /Channel/, "All channels");
    expect(onChange).toHaveBeenLastCalledWith({ ...DEFAULT_FILTERS, endpoint: "ep-lis" });
    await chooseOption(user, /Endpoint/, "All endpoints");
    expect(onChange).toHaveBeenLastCalledWith(DEFAULT_FILTERS);
  });

  it("keeps a channel that is no longer configured selectable, by its ID", () => {
    renderWithProviders(
      <Harness initial={{ ...DEFAULT_FILTERS, channel: "ch-gone" }} onChange={vi.fn()} />,
    );
    expect(screen.getByRole("button", { name: /Channel/ }).textContent).toContain("ch-gone");
  });

  it("sets the time range from Live, 15 minutes, an hour or a day", async () => {
    const onChange = vi.fn();
    const { user } = renderWithProviders(<Harness onChange={onChange} />);
    await chooseOption(user, /Time range/, "Last 15 minutes");
    expect(onChange).toHaveBeenLastCalledWith({ ...DEFAULT_FILTERS, range: "15m" });
    await chooseOption(user, /Time range/, "Last hour");
    expect(onChange).toHaveBeenLastCalledWith({ ...DEFAULT_FILTERS, range: "1h" });
    await chooseOption(user, /Time range/, "Last 24 hours");
    expect(onChange).toHaveBeenLastCalledWith({ ...DEFAULT_FILTERS, range: "24h" });
    await chooseOption(user, /Time range/, "Live");
    expect(onChange).toHaveBeenLastCalledWith(DEFAULT_FILTERS);
  });

  it("takes custom times, leaving half-typed ones alone, and drops them when a quick range is chosen", async () => {
    const onChange = vi.fn();
    const { user } = renderWithProviders(<Harness onChange={onChange} />);
    await chooseOption(user, /Time range/, "Custom range…");
    expect(onChange).toHaveBeenLastCalledWith({ ...DEFAULT_FILTERS, range: "custom" });
    const from = screen.getByLabelText("From");
    fireEvent.change(from, { target: { value: "" } });
    fireEvent.blur(from);
    expect(onChange).toHaveBeenCalledTimes(1);
    fireEvent.change(from, { target: { value: "2026-11-01T08:00" } });
    fireEvent.blur(from);
    expect(onChange).toHaveBeenLastCalledWith({
      ...DEFAULT_FILTERS,
      range: "custom",
      from: new Date("2026-11-01T08:00").toISOString(),
    });
    const to = screen.getByLabelText("To");
    fireEvent.change(to, { target: { value: "2026-11-02T08:00" } });
    fireEvent.keyDown(to, { key: "Enter" });
    expect(onChange).toHaveBeenLastCalledWith({
      ...DEFAULT_FILTERS,
      range: "custom",
      from: new Date("2026-11-01T08:00").toISOString(),
      to: new Date("2026-11-02T08:00").toISOString(),
    });
    await chooseOption(user, /Time range/, "Last hour");
    expect(onChange).toHaveBeenLastCalledWith({ ...DEFAULT_FILTERS, range: "1h" });
    expect(screen.queryByLabelText("From")).toBeNull();
  });

  it("searches the message text after a pause in typing, and at once on Enter", async () => {
    const onChange = vi.fn();
    const { user } = renderWithProviders(<Harness onChange={onChange} />);
    const text = screen.getByRole("searchbox", { name: "Search log messages" });
    await user.type(text, "no ack");
    expect(onChange).not.toHaveBeenCalled();
    await waitFor(() => expect(onChange).toHaveBeenCalledTimes(1));
    expect(onChange).toHaveBeenLastCalledWith({ ...DEFAULT_FILTERS, text: "no ack" });
    await user.clear(text);
    await user.type(text, "refused{Enter}");
    expect(onChange).toHaveBeenLastCalledWith({ ...DEFAULT_FILTERS, text: "refused" });
    await user.click(screen.getByRole("button", { name: "Clear search" }));
    expect(onChange).toHaveBeenLastCalledWith(DEFAULT_FILTERS);
  });

  it("filters by event name, typed or chosen from the catalog's names, and clears it", async () => {
    const onChange = vi.fn();
    const { user } = renderWithProviders(<Harness onChange={onChange} />);
    const event = screen.getByRole("combobox", { name: "Event" });
    const suggested = [...document.querySelectorAll("datalist option")].map((option) =>
      option.getAttribute("value"),
    );
    expect(suggested).toContain("channel.started");
    expect(suggested).toContain("delivery.state_changed");
    expect(suggested).toContain("operator.command");
    await user.type(event, "delivery.state_changed");
    expect(onChange).not.toHaveBeenCalled();
    await waitFor(() => expect(onChange).toHaveBeenCalledTimes(1));
    expect(onChange).toHaveBeenLastCalledWith({
      ...DEFAULT_FILTERS,
      event: "delivery.state_changed",
    });
    expect(screen.getByText("Clear filters")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Clear event filter" }));
    expect(onChange).toHaveBeenLastCalledWith(DEFAULT_FILTERS);
    await user.type(event, "channel.started{Enter}");
    expect(onChange).toHaveBeenLastCalledWith({ ...DEFAULT_FILTERS, event: "channel.started" });
  });

  it("shows the event from the filters, and Clear filters empties it", async () => {
    const onChange = vi.fn();
    const { user } = renderWithProviders(
      <Harness initial={{ ...DEFAULT_FILTERS, event: "config.applied" }} onChange={onChange} />,
    );
    expect(screen.getByRole("combobox", { name: "Event" })).toHaveProperty(
      "value",
      "config.applied",
    );
    await user.click(screen.getByRole("button", { name: "Clear filters" }));
    expect(screen.getByRole("combobox", { name: "Event" })).toHaveProperty("value", "");
  });

  it("shows Clear filters while anything differs from the defaults, and restores them", async () => {
    const onChange = vi.fn();
    const { user } = renderWithProviders(
      <Harness
        initial={{ level: "error", range: "1h", channel: "ch-chem", text: "x" }}
        onChange={onChange}
      />,
    );
    await user.click(screen.getByRole("button", { name: "Clear filters" }));
    expect(onChange).toHaveBeenLastCalledWith(DEFAULT_FILTERS);
    expect(screen.queryByRole("button", { name: "Clear filters" })).toBeNull();
    expect(screen.getByRole("button", { name: /Level/ }).textContent).toContain("Info and above");
  });

  it("is operable with the keyboard alone", async () => {
    const onChange = vi.fn();
    const { user } = renderWithProviders(<Harness onChange={onChange} />);
    await chooseWithKeyboard(user, /Level/, "Debug and above");
    expect(onChange).toHaveBeenLastCalledWith({ ...DEFAULT_FILTERS, level: "debug" });
    await chooseWithKeyboard(user, /Time range/, "Last hour");
    expect(onChange).toHaveBeenLastCalledWith({ ...DEFAULT_FILTERS, level: "debug", range: "1h" });
  });

  it("has no accessibility violations, with a custom range showing", async () => {
    const { container } = renderWithProviders(
      <Harness
        initial={{
          ...DEFAULT_FILTERS,
          range: "custom",
          from: "2026-11-01T08:00:00Z",
          level: "warn",
        }}
        onChange={vi.fn()}
      />,
    );
    await expectNoA11yViolations(container);
  });
});
