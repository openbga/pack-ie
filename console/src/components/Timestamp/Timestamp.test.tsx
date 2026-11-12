import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { LabTimeZoneContext } from "@/lib/format";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { Timestamp } from "./Timestamp";

describe("Timestamp", () => {
  it("shows UTC, named, when no laboratory time zone is provided", async () => {
    const { container } = render(<Timestamp value="2026-10-12T14:03:05Z" />);
    const time = screen.getByText("2026-10-12 14:03 UTC");
    expect(time.getAttribute("datetime")).toBe("2026-10-12T14:03:05Z");
    await expectNoA11yViolations(container);
  });

  it("shows the laboratory time zone with its name, and seconds when asked", () => {
    render(
      <LabTimeZoneContext.Provider value="America/Chicago">
        <Timestamp value="2026-10-12T14:03:05Z" seconds />
      </LabTimeZoneContext.Provider>,
    );
    expect(screen.getByText("2026-10-12 09:03:05 CDT")).toBeTruthy();
  });
});
