import { act, fireEvent, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Button } from "@/components/Button";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderWithProviders } from "@/lib/test/render";
import { useToast } from "./ToastProvider";

function Trigger({ tone }: { tone: "info" | "success" | "warning" | "danger" }) {
  const toast = useToast();
  return (
    <Button onPress={() => toast.show({ tone, title: "Printer not reachable" })}>Print</Button>
  );
}

const region = () => screen.getByRole("region", { name: "Notifications" });
const notice = () => within(region()).queryByText("Printer not reachable");

/** Shows a notice by pressing the trigger, with fake timers already running. */
function showNotice(tone: "info" | "success" | "warning" | "danger") {
  vi.useFakeTimers();
  renderWithProviders(<Trigger tone={tone} />);
  fireEvent.click(screen.getByRole("button", { name: "Print" }));
  const item = notice()?.closest("li");
  if (!item) throw new Error("no notice");
  return item;
}

describe("Toast", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("announces a notice in the live region until dismissed", async () => {
    const { user, container } = renderWithProviders(<Trigger tone="danger" />);
    expect(within(region()).getByRole("list").getAttribute("aria-live")).toBe("polite");
    await user.click(screen.getByRole("button", { name: "Print" }));
    expect(notice()).toBeTruthy();
    await expectNoA11yViolations(container);
    await user.click(within(region()).getByRole("button", { name: "Dismiss" }));
    expect(notice()).toBeNull();
  });

  it("lets success and information leave by themselves", () => {
    showNotice("success");
    act(() => vi.advanceTimersByTime(8000));
    expect(notice()).toBeNull();
  });

  it("keeps warnings and errors until dismissed", () => {
    showNotice("warning");
    act(() => vi.advanceTimersByTime(60_000));
    expect(notice()).toBeTruthy();
  });

  it("pauses while the pointer is over the notice or focus is in it", () => {
    const item = showNotice("info");
    act(() => vi.advanceTimersByTime(5000));
    fireEvent.pointerEnter(item);
    act(() => vi.advanceTimersByTime(60_000));
    expect(notice()).toBeTruthy();
    fireEvent.pointerLeave(item);
    fireEvent.focus(within(item).getByRole("button", { name: "Dismiss" }));
    act(() => vi.advanceTimersByTime(60_000));
    expect(notice()).toBeTruthy();
    fireEvent.blur(within(item).getByRole("button", { name: "Dismiss" }));
    // The time left before the pause, not a fresh 8 seconds.
    act(() => vi.advanceTimersByTime(2999));
    expect(notice()).toBeTruthy();
    act(() => vi.advanceTimersByTime(1));
    expect(notice()).toBeNull();
  });
});
