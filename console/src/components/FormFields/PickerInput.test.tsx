import { screen } from "@testing-library/react";
import { FormProvider, useForm } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderWithProviders } from "@/lib/test/render";
import { PickerInput } from "./PickerInput";

const OPTIONS = [
  { id: "a", label: "Synthetic endpoint A", description: "Serial · Connected" },
  { id: "b", label: "Synthetic endpoint B", description: "Folder · Disabled" },
];

interface Values {
  first: string | null;
  second: string | null;
}

function Harness({ onSubmit }: { onSubmit: (values: Values) => void }) {
  const form = useForm<Values>({ defaultValues: { first: null, second: null } });
  return (
    <FormProvider {...form}>
      <form aria-label="Synthetic form" onSubmit={form.handleSubmit(onSubmit)}>
        <PickerInput name="first" label="Side A" options={OPTIONS} isRequired />
        <PickerInput
          name="second"
          label="Side B"
          options={OPTIONS}
          isRequired
          validate={(value, values) => value !== values.first || "Side B must differ from side A."}
        />
        <button type="submit">Save</button>
      </form>
    </FormProvider>
  );
}

describe("PickerInput", () => {
  it("requires a choice, focuses the first empty picker and checks against other fields", async () => {
    const onSubmit = vi.fn();
    const { user, container } = renderWithProviders(<Harness onSubmit={onSubmit} />);
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Choose side a.")).toBeTruthy();
    expect(document.activeElement).toBe(screen.getByRole("combobox", { name: /Side A/ }));
    await expectNoA11yViolations(container);

    await user.type(screen.getByRole("combobox", { name: /Side A/ }), "endpoint a");
    await user.keyboard("{ArrowDown}{Enter}");
    await user.type(screen.getByRole("combobox", { name: /Side B/ }), "endpoint a");
    await user.keyboard("{ArrowDown}{Enter}");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Side B must differ from side A.")).toBeTruthy();
    expect(onSubmit).not.toHaveBeenCalled();

    const sideB = screen.getByRole("combobox", { name: /Side B/ });
    await user.clear(sideB);
    await user.type(sideB, "folder");
    await user.keyboard("{ArrowDown}{Enter}");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(onSubmit).toHaveBeenCalledWith({ first: "a", second: "b" }, expect.anything());
  });
});
