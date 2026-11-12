import { screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { TextField } from "@/components/TextField";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderWithProviders } from "@/lib/test/render";
import { ConfirmDialog } from "./ConfirmDialog";

function renderConfirm({ isPending = false } = {}) {
  const onConfirm = vi.fn();
  const onOpenChange = vi.fn();
  const view = renderWithProviders(
    <ConfirmDialog
      title="Discard batch?"
      isOpen
      onOpenChange={onOpenChange}
      confirmLabel="Discard batch"
      onConfirm={onConfirm}
      isPending={isPending}
    >
      <p>The synthetic batch will be discarded.</p>
      <TextField label="Batch code" name="code" value="" onChange={() => undefined} />
    </ConfirmDialog>,
  );
  return { ...view, onConfirm, onOpenChange };
}

describe("ConfirmDialog", () => {
  it("starts on Cancel, so Enter never confirms by accident", async () => {
    const { user, onConfirm, onOpenChange } = renderConfirm();
    const dialog = screen.getByRole("alertdialog", { name: "Discard batch?" });
    await expectNoA11yViolations(dialog);
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Cancel" }));
    await user.keyboard("{Enter}");
    expect(onConfirm).not.toHaveBeenCalled();
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("cancels on Escape, but not while the action runs", async () => {
    const { user, onOpenChange, unmount } = renderConfirm();
    await user.keyboard("{Escape}");
    expect(onOpenChange).toHaveBeenCalledWith(false);
    unmount();
    const running = renderConfirm({ isPending: true });
    await running.user.keyboard("{Escape}");
    expect(running.onOpenChange).not.toHaveBeenCalled();
  });

  it("confirms on Enter in a field, but not while the action runs", async () => {
    const { user, onConfirm, unmount } = renderConfirm();
    await user.click(screen.getByRole("textbox", { name: "Batch code" }));
    await user.keyboard("{Enter}");
    expect(onConfirm).toHaveBeenCalledOnce();
    unmount();
    const running = renderConfirm({ isPending: true });
    await running.user.click(screen.getByRole("textbox", { name: "Batch code" }));
    await running.user.keyboard("{Enter}");
    expect(running.onConfirm).not.toHaveBeenCalled();
  });

  it("confirms with the named action", async () => {
    const { user, onConfirm } = renderConfirm();
    await user.click(screen.getByRole("button", { name: "Discard batch" }));
    expect(onConfirm).toHaveBeenCalledOnce();
  });
});
