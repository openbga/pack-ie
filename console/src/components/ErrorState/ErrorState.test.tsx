import { screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { engineError } from "@/lib/test/engineError";
import { renderWithProviders } from "@/lib/test/render";
import { ErrorState } from "./ErrorState";

describe("ErrorState", () => {
  it("shows the message, the support reference and a retry", async () => {
    const onRetry = vi.fn();
    const error = engineError("internal", "The console could not complete the action.", {
      correlationId: "corr-42",
    });
    const { user, container } = renderWithProviders(<ErrorState error={error} onRetry={onRetry} />);
    const alert = screen.getByRole("alert");
    expect(alert.textContent).toContain("The console could not complete the action.");
    expect(alert.textContent).toContain("Support reference: corr-42");
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(onRetry).toHaveBeenCalledOnce();
    await expectNoA11yViolations(container);
  });

  it("omits an empty support reference", () => {
    renderWithProviders(
      <ErrorState error={engineError("unavailable", "Not available.", { correlationId: "" })} />,
    );
    expect(screen.getByRole("alert").textContent).not.toContain("Support reference");
  });

  it("falls back to a plain message when the engine sent none", () => {
    renderWithProviders(<ErrorState error={engineError("engine_unavailable", "")} />);
    expect(screen.getByRole("alert").textContent).toContain("The engine is not running.");
  });
});
