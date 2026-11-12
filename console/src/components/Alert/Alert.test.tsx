import { screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderWithProviders } from "@/lib/test/render";
import { Alert } from "./Alert";

describe("Alert", () => {
  it("announces errors at once and information politely", async () => {
    const { container } = renderWithProviders(
      <>
        <Alert tone="danger" title="Action failed">
          Synthetic error.
        </Alert>
        <Alert tone="info">Synthetic note.</Alert>
      </>,
    );
    expect(screen.getByRole("alert").textContent).toContain("Action failed");
    expect(screen.getByRole("status").textContent).toContain("Synthetic note.");
    await expectNoA11yViolations(container);
  });

  it("announces a hold at once, with its title", async () => {
    const { container } = renderWithProviders(
      <Alert tone="hold" title="Release is blocked">
        Synthetic identity hold.
      </Alert>,
    );
    const alert = screen.getByRole("alert");
    expect(alert.textContent).toContain("Release is blocked");
    expect(alert.getAttribute("data-tone")).toBe("hold");
    await expectNoA11yViolations(container);
  });

  it("shows a compact note list, centring the icon of an untitled alert", async () => {
    const { container } = renderWithProviders(
      <Alert tone="info" compact>
        <ul>
          <li>Synthetic first note.</li>
          <li>Synthetic second note.</li>
        </ul>
      </Alert>,
    );
    const note = screen.getByRole("status");
    expect(note.hasAttribute("data-compact")).toBe(true);
    expect(note.hasAttribute("data-titled")).toBe(false);
    expect(screen.getAllByRole("listitem")).toHaveLength(2);
    await expectNoA11yViolations(container);
  });

  it("keeps an alert on screen until the page removes it", async () => {
    vi.useFakeTimers();
    try {
      renderWithProviders(
        <Alert tone="danger" title="Label print failed">
          Synthetic error.
        </Alert>,
      );
      vi.advanceTimersByTime(10 * 60 * 1000);
      expect(screen.getByRole("alert").textContent).toContain("Label print failed");
    } finally {
      vi.useRealTimers();
    }
  });
});
