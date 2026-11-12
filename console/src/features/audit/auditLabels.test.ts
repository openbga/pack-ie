import { describe, expect, it } from "vitest";
import { ACTION_CHOICES, humanizeKey } from "./auditLabels";

describe("humanizeKey", () => {
  it("spells a key as words", () => {
    expect(humanizeKey("base_version")).toBe("Base version");
    expect(humanizeKey("note")).toBe("Note");
  });

  it("names the resend confirmations by what the operator confirmed", () => {
    expect(humanizeKey("confirm_superseded")).toBe("Superseded message confirmed");
    expect(humanizeKey("confirm_duplicate_risk")).toBe("Duplicate risk confirmed");
  });
});

describe("action labels", () => {
  it("label every action of the catalog, the new ones included", () => {
    const ids = ACTION_CHOICES.map((choice) => choice.id);
    expect(ids).toEqual(expect.arrayContaining(["message_export", "transmission_verify"]));
    for (const choice of ACTION_CHOICES) expect(choice.label.trim()).not.toBe("");
  });
});
