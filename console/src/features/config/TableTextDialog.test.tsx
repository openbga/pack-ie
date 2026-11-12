import { screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { openResults, textbox } from "./testActions";
import { stubApi } from "./testFixtures";

describe("code table as text", () => {
  it("fills a table from pasted lines after saying what it would do", async () => {
    const { user } = await openResults(stubApi());
    await user.click(screen.getByRole("button", { name: "Add destination 1 code table" }));
    await user.click(screen.getByRole("button", { name: "Edit destination 1 table 1 as text" }));
    const dialog = await screen.findByRole("dialog", {
      name: "Edit destination 1 table 1 as text",
    });
    await expectNoA11yViolations(dialog);
    await user.click(textbox(dialog, /^Rows/));
    await user.paste("M\tMale\nF\tFemale\nbad line");
    expect(within(dialog).getByRole("alert").textContent).toContain("Line 3");
    expect(
      (within(dialog).getByRole("button", { name: "Replace rows" }) as HTMLButtonElement).disabled,
    ).toBe(true);
    await user.clear(textbox(dialog, /^Rows/));
    await user.paste("M\tMale\nF\tFemale");
    expect(within(dialog).getByRole("status").textContent).toBe(
      "2 rows: 2 new, 0 changed, 0 removed, 0 unchanged.",
    );
    await user.click(within(dialog).getByRole("button", { name: "Replace rows" }));
    expect(
      (
        textbox(
          screen.getByRole("group", { name: "Row 2 of destination 1 table 1" }),
          /^Text/,
        ) as HTMLInputElement
      ).value,
    ).toBe("Female");
  });
});
