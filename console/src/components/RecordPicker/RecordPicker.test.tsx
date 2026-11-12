import { screen, within } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderWithProviders } from "@/lib/test/render";
import { type RecordOption, RecordPicker } from "./RecordPicker";

const ENDPOINTS: RecordOption[] = [
  { id: "acl-top", label: "ACL TOP", description: "Serial · Disconnected" },
  { id: "acl-top-2", label: "ACL TOP", description: "TCP client · Connected" },
  { id: "lis-out", label: "LIS HL7 out", description: "TCP client · Connected" },
];

function Harness({
  onChange,
  initial = null,
}: {
  onChange: (value: string | null) => void;
  initial?: string | null;
}) {
  const [value, setValue] = useState<string | null>(initial);
  return (
    <RecordPicker
      label="Endpoint"
      name="endpointId"
      options={ENDPOINTS}
      value={value}
      onChange={(next) => {
        setValue(next);
        onChange(next);
      }}
    />
  );
}

const field = () => screen.getByRole("combobox", { name: "Endpoint" });
const optionTexts = () =>
  within(screen.getByRole("listbox"))
    .queryAllByRole("option")
    .map((option) => option.textContent);

describe("RecordPicker", () => {
  it("narrows by typing and keeps near-duplicates apart by their description", async () => {
    const onChange = vi.fn();
    const { user, container } = renderWithProviders(<Harness onChange={onChange} />);
    expect(field()).toHaveProperty("value", "");
    await expectNoA11yViolations(container);
    await user.type(field(), "acl");
    expect(optionTexts()).toEqual([
      "ACL TOPSerial · Disconnected",
      "ACL TOPTCP client · Connected",
    ]);
    await user.clear(field());
    await user.type(field(), "disconnected");
    expect(optionTexts()).toEqual(["ACL TOPSerial · Disconnected"]);
    await user.keyboard("{ArrowDown}{Enter}");
    expect(onChange).toHaveBeenLastCalledWith("acl-top");
    expect(field()).toHaveProperty("value", "ACL TOP (Serial · Disconnected)");
  });

  it("shows the current choice, marked in the list, and opens the full list on a click", async () => {
    const { user } = renderWithProviders(<Harness onChange={vi.fn()} initial="lis-out" />);
    expect(field()).toHaveProperty("value", "LIS HL7 out (TCP client · Connected)");
    await user.click(field());
    expect(optionTexts()).toHaveLength(ENDPOINTS.length);
    expect(
      within(screen.getByRole("listbox")).getByRole("option", { selected: true }).textContent,
    ).toContain("LIS HL7 out");
  });

  it("opens on a click but not on keyboard focus, and is chosen with the arrow keys", async () => {
    const onChange = vi.fn();
    const { user } = renderWithProviders(<Harness onChange={onChange} />);
    await user.tab();
    expect(document.activeElement).toBe(field());
    expect(screen.queryByRole("listbox")).toBeNull();
    await user.keyboard("{ArrowDown}{ArrowDown}{Enter}");
    expect(onChange).toHaveBeenLastCalledWith("acl-top-2");
  });

  it("says when nothing matches and keeps no free text", async () => {
    const onChange = vi.fn();
    const { user } = renderWithProviders(<Harness onChange={onChange} />);
    await user.type(field(), "nothing");
    expect(screen.getByRole("listbox").textContent).toBe("No matches");
    await user.tab();
    expect(field()).toHaveProperty("value", "");
    expect(onChange).not.toHaveBeenCalled();
  });
});
