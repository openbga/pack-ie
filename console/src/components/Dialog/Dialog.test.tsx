import { screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import { Button } from "@/components/Button";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderWithProviders } from "@/lib/test/render";
import { Dialog } from "./Dialog";

function Harness() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button onPress={() => setOpen(true)}>Open</Button>
      <Dialog
        title="Specimen details"
        isOpen={open}
        onOpenChange={setOpen}
        actions={<Button onPress={() => setOpen(false)}>Close</Button>}
      >
        Synthetic content
      </Dialog>
    </>
  );
}

describe("Dialog", () => {
  it("opens with a title, closes on Escape and returns focus", async () => {
    const { user } = renderWithProviders(<Harness />);
    const trigger = screen.getByRole("button", { name: "Open" });
    await user.click(trigger);
    const dialog = screen.getByRole("dialog", { name: "Specimen details" });
    await expectNoA11yViolations(dialog);
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });

  it("removes live-region announcements that name what left with it", async () => {
    // What React Aria leaves behind when a pending button goes with its dialog.
    const announcer = document.createElement("div");
    announcer.dataset.liveAnnouncer = "true";
    document.body.append(announcer);
    const note = (labelledby: string) => {
      const node = document.createElement("div");
      node.setAttribute("role", "img");
      node.setAttribute("aria-labelledby", labelledby);
      announcer.append(node);
      return node;
    };
    const { user } = renderWithProviders(<Harness />);
    const trigger = screen.getByRole("button", { name: "Open" });
    await user.click(trigger);
    const close = screen.getByRole("button", { name: "Close" });
    const namesClose = note(close.id);
    const namesTrigger = note(trigger.id);
    await user.click(close);
    expect(namesClose.isConnected).toBe(false);
    expect(namesTrigger.isConnected).toBe(true);
    // One that arrives a moment later (the announcer is created late) goes too.
    const late = note(close.id);
    await waitFor(() => expect(late.isConnected).toBe(false));
    announcer.remove();
  });
});
