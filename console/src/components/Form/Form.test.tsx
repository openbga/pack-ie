import { screen } from "@testing-library/react";
import { useEffect, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { describe, expect, it, vi } from "vitest";
import { Button } from "@/components/Button";
import { TextField } from "@/components/TextField";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { engineError } from "@/lib/test/engineError";
import { renderWithProviders } from "@/lib/test/render";
import { Form } from "./Form";

describe("Form", () => {
  it("submits", async () => {
    const onSubmit = vi.fn((event: { preventDefault: () => void }) => event.preventDefault());
    const { user, container } = renderWithProviders(
      <Form
        aria-label="Receive"
        stickyActions
        onSubmit={onSubmit}
        actions={<Button type="submit">Receive</Button>}
      >
        <TextField label="Accession" name="accession" value="SYN-1" onChange={() => undefined} />
      </Form>,
    );
    await user.click(screen.getByRole("button", { name: "Receive" }));
    expect(onSubmit).toHaveBeenCalledOnce();
    await expectNoA11yViolations(container);
  });

  it("shows an engine error and its field errors on the matching field", async () => {
    const error = engineError("validation", "Check the highlighted fields.", {
      fieldErrors: [{ field: "accession", message: "This accession is already received." }],
    });
    const { container } = renderWithProviders(
      <Form
        aria-label="Receive"
        onSubmit={() => undefined}
        error={error}
        errorTitle="Receipt failed"
        actions={<Button type="submit">Receive</Button>}
      >
        <TextField label="Accession" name="accession" value="SYN-1" onChange={() => undefined} />
      </Form>,
    );
    expect(screen.getByRole("alert").textContent).toContain("Receipt failed");
    const field = screen.getByRole("textbox", { name: "Accession" });
    expect(field.getAttribute("aria-invalid")).toBe("true");
    // Focus moves to the first invalid field.
    expect(document.activeElement).toBe(field);
    expect(screen.getByText("This accession is already received.")).toBeTruthy();
    await expectNoA11yViolations(container);
  });

  it("shows a field error that no field on the form shows in place of the generic message", () => {
    const error = engineError("validation", "Check the highlighted fields.", {
      fieldErrors: [
        {
          field: "instrumentId",

          message: "The source format must match the instrument connection.",
        },
      ],
    });
    renderWithProviders(
      <Form
        aria-label="Register source"
        onSubmit={() => undefined}
        error={error}
        errorTitle="Not recorded"
        actions={<Button type="submit">Register source</Button>}
      >
        <TextField
          label="Sender name"
          name="senderName"
          value="AU-SIM"
          onChange={() => undefined}
        />
      </Form>,
    );
    const alert = screen.getByRole("alert");
    expect(alert.textContent).toContain("The source format must match the instrument connection.");
    expect(alert.textContent).not.toContain("Check the highlighted fields.");
    expect(screen.getByRole("textbox", { name: "Sender name" }).getAttribute("aria-invalid")).toBe(
      null,
    );
  });

  it("keeps the generic message beside a field error no field shows when another is highlighted", () => {
    const error = engineError("validation", "Check the highlighted fields.", {
      fieldErrors: [
        { field: "senderName", message: "Enter the sender name." },
        { field: "instrumentId", message: "The instrument is retired." },
      ],
    });
    renderWithProviders(
      <Form
        aria-label="Register source"
        onSubmit={() => undefined}
        error={error}
        actions={<Button type="submit">Register source</Button>}
      >
        <TextField label="Sender name" name="senderName" value="" onChange={() => undefined} />
      </Form>,
    );
    const alert = screen.getByRole("alert");
    expect(alert.textContent).toContain("Check the highlighted fields. The instrument is retired.");
    expect(alert.textContent).not.toContain("Enter the sender name.");
  });

  it("hides a field's engine error as soon as the field is edited", async () => {
    const error = engineError("validation", "Check the highlighted fields.", {
      fieldErrors: [
        {
          field: "npi",

          message: "This NPI is not valid. Please check the number for a typo.",
        },
        { field: "familyName", message: "Enter family name." },
      ],
    });
    function Fields() {
      const [npi, setNpi] = useState("1234567890");
      return (
        <Form
          aria-label="Register"
          onSubmit={() => undefined}
          error={error}
          actions={<Button type="submit">Register</Button>}
        >
          <TextField label="NPI" name="npi" value={npi} onChange={setNpi} />
          <TextField label="Family name" name="familyName" value="" onChange={() => undefined} />
        </Form>
      );
    }
    const { user } = renderWithProviders(<Fields />);
    const npi = screen.getByRole("textbox", { name: "NPI" });
    await user.clear(npi);
    await user.type(npi, "1234567893");
    // The field keeps focus: the error goes away while typing, not on blur.
    expect(document.activeElement).toBe(npi);
    expect(
      screen.queryByText("This NPI is not valid. Please check the number for a typo."),
    ).toBeNull();
    expect(npi.getAttribute("aria-invalid")).toBeNull();
    expect(screen.getByText("Enter family name.")).toBeTruthy();
  });

  // #4485: tracking edits re-rendered the form before React saw a field's change, which undid
  // the change, so the first character typed in a field was lost.
  it("keeps the first character typed in each field", async () => {
    function Fields() {
      const [values, setValues] = useState({ accession: "", reason: "" });
      const outer = useRef<HTMLDivElement>(null);
      // A browser runs pending React work between event listeners; jsdom does not.
      useEffect(() => {
        outer.current?.addEventListener("input", () => flushSync(() => undefined));
      }, []);
      return (
        <div ref={outer}>
          <Form aria-label="Correct" onSubmit={() => undefined} actions={null}>
            {(["accession", "reason"] as const).map((name) => (
              <TextField
                key={name}
                label={name}
                name={name}
                value={values[name]}
                onChange={(value) => setValues((last) => ({ ...last, [name]: value }))}
              />
            ))}
          </Form>
        </div>
      );
    }
    const { user } = renderWithProviders(<Fields />);
    for (const name of ["accession", "reason"]) {
      const field = screen.getByRole<HTMLInputElement>("textbox", { name });
      await user.click(field);
      await user.keyboard("Test reason");
      expect(field.value).toBe("Test reason");
    }
  });
});
