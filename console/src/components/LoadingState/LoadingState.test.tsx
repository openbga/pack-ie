import { screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderWithProviders } from "@/lib/test/render";
import { LoadingState } from "./LoadingState";

describe("LoadingState", () => {
  it("announces what is loading", async () => {
    const { container } = renderWithProviders(<LoadingState label="Loading worklist…" />);
    expect(screen.getByRole("status").textContent).toBe("Loading worklist…");
    expect(screen.queryByTestId("skeleton")).toBeNull();
    await expectNoA11yViolations(container);
  });

  it("reserves the space of the rows to come, hidden from assistive technology", async () => {
    const { container } = renderWithProviders(<LoadingState label="Loading worklist…" rows={8} />);
    expect(screen.getByRole("status").textContent).toBe("Loading worklist…");
    expect(screen.getByTestId("skeleton").getAttribute("aria-hidden")).toBe("true");
    await expectNoA11yViolations(container);
  });
});
