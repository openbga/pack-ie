import { screen } from "@testing-library/react";
import { FormProvider, useForm } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { chooseWithKeyboard } from "@/lib/test/keyboard";
import { renderWithProviders } from "@/lib/test/render";
import { ChoiceInput } from "./ChoiceInput";
import { type TextFormat, TextInput } from "./TextInput";

const DATE_FORMAT: TextFormat = {
  pattern: /^\d{4}-\d{2}-\d{2}$/,
  message: "Enter the date as YYYY-MM-DD.",
  description: "YYYY-MM-DD",
};

interface Values {
  code: string;
  date: string;
  matrix: string | null;
  site: string | null;
}

function Harness({ onSubmit }: { onSubmit: (values: Values) => void }) {
  const form = useForm<Values>({
    defaultValues: { code: "", date: "", matrix: null, site: null },
  });
  return (
    <FormProvider {...form}>
      <form aria-label="Synthetic form" onSubmit={form.handleSubmit(onSubmit)}>
        <TextInput name="code" label="Code" isRequired />
        <TextInput name="date" label="Collection date" format={DATE_FORMAT} />
        <ChoiceInput
          name="matrix"
          label="Matrix"
          options={[{ id: "urine", label: "Urine" }]}
          isRequired
        />
        <ChoiceInput name="site" label="Site" options={[]} isDisabled />
        <button type="submit">Save</button>
      </form>
    </FormProvider>
  );
}

describe("FormFields", () => {
  it("checks presence and format before submitting, then submits the typed values", async () => {
    const onSubmit = vi.fn();
    const { user, container } = renderWithProviders(<Harness onSubmit={onSubmit} />);
    await expectNoA11yViolations(container);
    await user.type(screen.getByRole("textbox", { name: /Collection date/ }), "12/10/2026");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Enter code.")).toBeTruthy();
    expect(screen.getByText("Enter the date as YYYY-MM-DD.")).toBeTruthy();
    expect(screen.getByText("Choose matrix.")).toBeTruthy();
    expect(onSubmit).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(screen.getByRole("textbox", { name: /Code/ }));

    await user.type(screen.getByRole("textbox", { name: /Code/ }), "SYN-1");
    await user.clear(screen.getByRole("textbox", { name: /Collection date/ }));
    await user.type(screen.getByRole("textbox", { name: /Collection date/ }), "2026-10-12");
    await user.click(screen.getByRole("button", { name: "Save" }));
    // A choice takes focus too when it is the first invalid field.
    expect(document.activeElement).toBe(screen.getByRole("button", { name: /Matrix$/ }));
    await chooseWithKeyboard(user, /Matrix$/, "Urine");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(onSubmit).toHaveBeenCalledWith(
      { code: "SYN-1", date: "2026-10-12", matrix: "urine", site: null },
      expect.anything(),
    );
  });

  it("disables a choice that has nothing to choose yet", () => {
    renderWithProviders(<Harness onSubmit={vi.fn()} />);
    expect(screen.getByRole("button", { name: /Site$/ })).toHaveProperty("disabled", true);
  });
});
