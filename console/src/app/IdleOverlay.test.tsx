import { act, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Button } from "@/components/Button";
import { Dialog } from "@/components/Dialog";
import { IdleProvider, useOnIdle } from "@/lib/idle";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderWithProviders } from "@/lib/test/render";
import { IdleOverlay } from "./IdleOverlay";

/** A page with a heading, a button that goes with the content on idle, and an open dialog. */
function Console({ withDialog = false }: { withDialog?: boolean }) {
  const [content, setContent] = useState(true);
  useOnIdle(() => setContent(false));
  return (
    <>
      <main>
        <h1>Messages</h1>
        {content && <Button>Show body</Button>}
        <Button>Other action</Button>
      </main>
      {withDialog && (
        <Dialog title="Export" isOpen onOpenChange={() => {}} actions={<Button>Cancel</Button>}>
          Typed text
        </Dialog>
      )}
    </>
  );
}

function setup(withDialog = false) {
  return renderWithProviders(
    <IdleProvider>
      <Console withDialog={withDialog} />
      <IdleOverlay />
    </IdleProvider>,
  );
}

/** React Aria hides with `inert` where the browser has it, else with aria-hidden (jsdom). */
const isHidden = (element: HTMLElement) =>
  element.hasAttribute("inert") || element.getAttribute("aria-hidden") === "true";

const goIdle = () => act(() => void vi.advanceTimersByTime(15 * 60_000 + 1000));

describe("IdleOverlay", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.useFakeTimers({ shouldAdvanceTime: true });
  });
  afterEach(() => vi.useRealTimers());

  it("covers the console after the idle time, with focus on Continue", async () => {
    const { container } = setup();
    expect(screen.queryByRole("alertdialog")).toBeNull();
    goIdle();
    const cover = await screen.findByRole("alertdialog", { name: "Content hidden" });
    expect(cover.textContent).toContain(
      "Content hidden after 15 minutes without activity. Choose Continue to carry on.",
    );
    const carryOn = screen.getByRole("button", { name: "Continue" });
    await waitFor(() => expect(document.activeElement).toBe(carryOn));
    // The console beneath is out of reach of keyboard and assistive technology.
    expect(isHidden(container)).toBe(true);
    expect(screen.queryByRole("heading", { name: "Messages" })).toBeNull();
    await expectNoA11yViolations(cover);
  });

  it("keeps Tab and Escape inside the cover, and Enter on Continue dismisses it", async () => {
    const { user, container } = setup();
    goIdle();
    const carryOn = await screen.findByRole("button", { name: "Continue" });
    await waitFor(() => expect(document.activeElement).toBe(carryOn));
    await user.tab();
    await user.keyboard("{Escape}");
    expect(document.activeElement).toBe(carryOn);
    expect(screen.getByRole("alertdialog", { name: "Content hidden" })).toBeTruthy();
    await user.keyboard("{Enter}");
    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
    expect(isHidden(container)).toBe(false);
  });

  it("returns focus to the heading when what had focus is gone", async () => {
    const { user } = setup();
    screen.getByRole("button", { name: "Show body" }).focus();
    goIdle();
    await user.click(await screen.findByRole("button", { name: "Continue" }));
    expect(screen.queryByRole("button", { name: "Show body" })).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole("heading")));
  });

  it("returns focus to the control that had it, when it is still there", async () => {
    const { user } = setup();
    const other = screen.getByRole("button", { name: "Other action" });
    other.focus();
    goIdle();
    await user.click(await screen.findByRole("button", { name: "Continue" }));
    await waitFor(() => expect(document.activeElement).toBe(other));
  });

  it("covers a dialog that is open, which is usable again after Continue", async () => {
    const { user } = setup(true);
    expect(screen.getByRole("dialog", { name: "Export" })).toBeTruthy();
    goIdle();
    await screen.findByRole("alertdialog", { name: "Content hidden" });
    // Only the cover is in the accessibility tree; the dialog keeps its text beneath it.
    expect(screen.queryByRole("dialog", { name: "Export" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await waitFor(() => expect(screen.getByRole("dialog", { name: "Export" })).toBeTruthy());
    expect(screen.getByText("Typed text")).toBeTruthy();
  });
});
