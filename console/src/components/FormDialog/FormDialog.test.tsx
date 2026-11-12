import { screen } from "@testing-library/react";
import { useForm } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";
import type { EngineError } from "@/api/types";
import { Button } from "@/components/Button";
import { TextInput } from "@/components/FormFields";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { engineError } from "@/lib/test/engineError";
import { renderWithProviders } from "@/lib/test/render";
import { FormDialog } from "./FormDialog";

interface HarnessProps {
  onSave: (reason: string) => void;
  onClose: () => void;
  error?: EngineError | null;
  isPending?: boolean;
  footnote?: string;
}

function Harness({ onSave, onClose, error = null, isPending = false, footnote }: HarnessProps) {
  const form = useForm({ defaultValues: { reason: "" } });
  return (
    <FormDialog
      title="Discard batch"
      description="Removes the batch from the bench."
      footnote={footnote}
      submitLabel="Discard batch"
      form={form}
      onSubmit={form.handleSubmit(({ reason }) => onSave(reason))}
      command={{ error, isPending }}
      errorTitle="The batch was not discarded"
      onClose={onClose}
      extraActions={<Button>Reload batch</Button>}
    >
      <TextInput name="reason" label="Reason" isRequired />
    </FormDialog>
  );
}

describe("FormDialog", () => {
  it("submits its form, with the fields reached through the form context", async () => {
    const onSave = vi.fn();
    const { user } = renderWithProviders(<Harness onSave={onSave} onClose={vi.fn()} />);
    const dialog = screen.getByRole("dialog", { name: "Discard batch" });
    await expectNoA11yViolations(dialog);
    await user.click(screen.getByRole("button", { name: "Discard batch" }));
    expect(await screen.findByText("Enter reason.")).toBeTruthy();
    expect(onSave).not.toHaveBeenCalled();
    await user.type(screen.getByRole("textbox", { name: /Reason/ }), "Expired reagent{Enter}");
    expect(onSave).toHaveBeenCalledWith("Expired reagent");
    expect(screen.getByRole("button", { name: "Reload batch" })).toBeTruthy();
  });

  it("puts a footnote first and the submit button last", () => {
    renderWithProviders(
      <Harness onSave={vi.fn()} onClose={vi.fn()} footnote="Nothing is discarded yet." />,
    );
    const dialog = screen.getByRole("dialog", { name: "Discard batch" });
    expect(screen.getByText("Removes the batch from the bench.")).toBeTruthy();
    expect(screen.getByText("Nothing is discarded yet.")).toBeTruthy();
    const buttons = Array.from(dialog.querySelectorAll("button")).map((b) => b.textContent);
    expect(buttons.slice(-3)).toEqual(["Cancel", "Reload batch", "Discard batch"]);
  });

  it("shows the engine's refusal and closes on Cancel", async () => {
    const onClose = vi.fn();
    const { user } = renderWithProviders(
      <Harness
        onSave={vi.fn()}
        onClose={onClose}
        error={engineError("conflict", "The batch changed.")}
      />,
    );
    expect(screen.getByRole("alert").textContent).toContain("The batch was not discarded");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("closes on Escape only while nothing is typed and no command runs", async () => {
    const onClose = vi.fn();
    const { user, unmount } = renderWithProviders(<Harness onSave={vi.fn()} onClose={onClose} />);
    await user.type(screen.getByRole("textbox", { name: /Reason/ }), "Expired");
    await user.keyboard("{Escape}");
    expect(onClose).not.toHaveBeenCalled();
    await user.clear(screen.getByRole("textbox", { name: /Reason/ }));
    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledOnce();
    unmount();

    const whileRunning = vi.fn();
    const running = renderWithProviders(
      <Harness onSave={vi.fn()} onClose={whileRunning} isPending />,
    );
    await running.user.keyboard("{Escape}");
    expect(whileRunning).not.toHaveBeenCalled();
  });
});
