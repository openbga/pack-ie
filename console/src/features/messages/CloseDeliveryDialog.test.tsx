import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Delivery, DeliveryState } from "@/api/types";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { engineError } from "@/lib/test/engineError";
import { renderWithProviders } from "@/lib/test/render";
import { renderRoutes } from "@/lib/test/router";
import { CloseDeliveryDialog } from "./CloseDeliveryDialog";
import { attempt, delivery, detail, stubApi, summary } from "./fixtures";
import { MessagePage } from "./MessagePage";

const open = (api = stubApi(), onClose = vi.fn(), overrides: Partial<Delivery> = {}) => {
  const { id, destinationLabel, state, ackLevel, attemptLog } = delivery("LIS", {
    state: "failed",
    ackLevel: "none",
    attemptLog: [attempt(1, { result: "write_failed", ackCode: null, ackLevel: "none" })],
    ...overrides,
  });
  const rendered = renderWithProviders(
    <CloseDeliveryDialog
      delivery={{ id, destinationLabel, state, ackLevel, attemptLog }}
      onClose={onClose}
    />,
    { api },
  );
  return { ...rendered, api, onClose };
};

const dialog = () => screen.getByRole("dialog", { name: "Close delivery to LIS" });
const reason = () => within(dialog()).getByRole("textbox", { name: /Reason/ }) as HTMLInputElement;

describe("CloseDeliveryDialog", () => {
  it("says that closing does not contact the destination, and has no violations", async () => {
    const { container } = open();
    expect(within(dialog()).getByText("Closing does not contact the destination.")).toBeTruthy();
    expect(within(dialog()).getByText(/The message is not\s+delivered to LIS/)).toBeTruthy();
    expect(within(dialog()).getByText("0 of 500 characters")).toBeTruthy();
    await expectNoA11yViolations(container);
  });

  it.each(["failed", "rejected", "queued"] as const)(
    "says a %s delivery was not delivered, and that closing contacts no one",
    (state) => {
      open(stubApi(), vi.fn(), { state, ackLevel: "none" });
      const text = dialog().textContent ?? "";
      expect(text).toContain("Closing does not contact the destination.");
      expect(text).toMatch(/The message is not\s+delivered to LIS/);
      expect(text).not.toContain("may have reached");
    },
  );

  describe.each(["timed_out", "outcome_unknown"] as const)("a %s delivery", (state) => {
    it("does not say it was not delivered, and says what may have happened", async () => {
      const { container } = open(stubApi(), vi.fn(), {
        state,
        ackLevel: "none",
        attemptLog: [attempt(1, { result: "ack_timeout", ackCode: null, ackLevel: "none" })],
      });
      const text = (dialog().textContent ?? "").replace(/\s+/g, " ");
      expect(text).toContain("Closing does not contact the destination.");
      expect(text).toContain(
        "Closing stops further attempts and does not contact LIS. The earlier attempt may have reached it: no acknowledgement was received.",
      );
      expect(text).not.toMatch(/not\s+delivered/);
      await expectNoA11yViolations(container);
    });

    it("quotes the last acknowledgement code that was received", () => {
      open(stubApi(), vi.fn(), {
        state,
        ackLevel: "received",
        attemptLog: [
          attempt(1, { result: "rejected", ackCode: "AE", ackLevel: "received" }),
          attempt(2, { result: "connection_lost_after_send", ackCode: null, ackLevel: "none" }),
        ],
      });
      expect((dialog().textContent ?? "").replace(/\s+/g, " ")).toContain(
        "The earlier attempt may have reached it: the last acknowledgement code was AE.",
      );
    });

    it("says what the acknowledgement level proved when no code was kept", () => {
      open(stubApi(), vi.fn(), { state, ackLevel: "handed_off", attemptLog: [] });
      expect((dialog().textContent ?? "").replace(/\s+/g, " ")).toContain(
        "may have reached it: it was written to the destination, which did not confirm it.",
      );
    });
  });

  it("requires a reason, and counts its characters", async () => {
    const { user, api } = open();
    await user.click(within(dialog()).getByRole("button", { name: "Close delivery" }));
    expect(await within(dialog()).findByText("Enter reason.")).toBeTruthy();
    expect(api.closeDelivery).not.toHaveBeenCalled();
    await user.type(reason(), "Entered by hand");
    expect(within(dialog()).getByText("15 of 500 characters")).toBeTruthy();
  });

  it("refuses a reason of more than 500 characters, and takes one of exactly 500", async () => {
    const { user, api } = open();
    fireEvent.change(reason(), { target: { value: "x".repeat(501) } });
    expect(within(dialog()).getByText("501 of 500 characters")).toBeTruthy();
    await user.click(within(dialog()).getByRole("button", { name: "Close delivery" }));
    expect(await within(dialog()).findByText("Use 500 characters or fewer.")).toBeTruthy();
    expect(api.closeDelivery).not.toHaveBeenCalled();
    fireEvent.change(reason(), { target: { value: "x".repeat(500) } });
    await user.click(within(dialog()).getByRole("button", { name: "Close delivery" }));
    await waitFor(() => expect(api.closeDelivery).toHaveBeenCalledWith("d-LIS", "x".repeat(500)));
  });

  it("closes the delivery with the trimmed reason, says so in words, and closes", async () => {
    const { user, api, onClose } = open();
    await user.type(reason(), "  Entered by hand  ");
    await user.click(within(dialog()).getByRole("button", { name: "Close delivery" }));
    await waitFor(() => expect(api.closeDelivery).toHaveBeenCalledWith("d-LIS", "Entered by hand"));
    expect(await screen.findByText("Delivery to LIS closed")).toBeTruthy();
    expect(screen.getByText("The destination was not contacted.")).toBeTruthy();
    expect(onClose).toHaveBeenCalled();
  });

  it("shows the engine's refusal in its words and stays open", async () => {
    const api = stubApi({
      closeDelivery: async () => {
        throw engineError("conflict", "A delivery that is sending cannot be closed.");
      },
    });
    const { user, onClose } = open(api);
    await user.type(reason(), "Entered by hand");
    await user.click(within(dialog()).getByRole("button", { name: "Close delivery" }));
    expect(await within(dialog()).findByText("The delivery was not closed")).toBeTruthy();
    expect(within(dialog()).getByText("A delivery that is sending cannot be closed.")).toBeTruthy();
    expect(onClose).not.toHaveBeenCalled();
    expect(reason().value).toBe("Entered by hand");
  });

  it("closes on Cancel without asking the engine", async () => {
    const { user, api, onClose } = open();
    await user.click(within(dialog()).getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalled();
    expect(api.closeDelivery).not.toHaveBeenCalled();
  });

  describe("from the message", () => {
    const ROUTES = [{ path: "/messages/:id", element: <MessagePage /> }];
    const states: DeliveryState[] = [
      "queued",
      "sending",
      "acknowledged",
      "rejected",
      "timed_out",
      "outcome_unknown",
      "failed",
      "cancelled",
    ];

    it("offers Close delivery… exactly for the states the spec allows", async () => {
      const api = stubApi({
        getMessage: async () =>
          detail({
            summary: summary(1),
            deliveries: states.map((state) => delivery(state, { state })),
          }),
      });
      renderRoutes(ROUTES, "/messages/msg-000001", { api });
      await screen.findByRole("heading", { level: 2, name: "Deliveries" });
      const closable = states.filter(
        (state) => screen.queryByRole("button", { name: `Close delivery to ${state}` }) !== null,
      );
      expect(closable).toEqual(["queued", "rejected", "timed_out", "outcome_unknown", "failed"]);
    });

    it("closes a delivery with a reason and reads the message again", async () => {
      const api = stubApi({
        getMessage: async () =>
          detail({
            summary: summary(1, { deliverySummary: "attention" }),
            deliveries: [
              delivery("LIS", { state: "rejected", ackLevel: "none" }),
              delivery("Archive"),
            ],
          }),
      });
      const { user } = renderRoutes(ROUTES, "/messages/msg-000001", { api });
      await screen.findByRole("heading", { level: 2, name: "Deliveries" });
      const reads = api.getMessage.mock.calls.length;
      await user.click(screen.getByRole("button", { name: "Close delivery to LIS" }));
      await user.type(reason(), "Rejected by the partner; entered by hand");
      await user.click(within(dialog()).getByRole("button", { name: "Close delivery" }));
      await waitFor(() =>
        expect(api.closeDelivery).toHaveBeenCalledWith(
          "d-LIS",
          "Rejected by the partner; entered by hand",
        ),
      );
      await waitFor(() => expect(api.getMessage.mock.calls.length).toBeGreaterThan(reads));
      await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    });
  });
});
