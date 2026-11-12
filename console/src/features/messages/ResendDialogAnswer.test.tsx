import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { ResendResult } from "@/api/types";
import { engineError } from "@/lib/test/engineError";
import { renderRoutes } from "@/lib/test/router";
import { delivery, detail, stubApi, summary } from "./fixtures";
import { MessagePage } from "./MessagePage";
import { accepted, box, dialog, open, subject, submit } from "./resendDialogSetup";

describe("ResendDialog", () => {
  describe("the engine's answer", () => {
    const refusal: ResendResult = {
      newMessageId: null,
      destinations: [
        accepted("dest-LIS", "LIS"),
        {
          destinationId: "dest-Archive",
          destinationLabel: "Archive",
          deliveryId: null,
          rejected: {
            code: "duplicate_risk_unconfirmed",
            message: "Check the destination and confirm the risk of duplicate delivery.",
          },
        },
        {
          destinationId: "dest-Review",
          destinationLabel: "Review",
          deliveryId: null,
          rejected: {
            code: "delivery_in_progress",
            message: "Wait for the existing delivery to finish.",
          },
        },
        {
          destinationId: "dest-Gone",
          destinationLabel: "Gone",
          deliveryId: null,
          rejected: {
            code: "destination_not_found",
            message: "Destination is absent or disabled in the active configuration.",
          },
        },
      ],
    };

    it("shows the engine's text next to the right destination, and goes back to the choices", async () => {
      const api = stubApi({ resendMessage: async () => refusal });
      const { user, onClose } = open(
        subject([
          ["LIS", "failed"],
          ["Archive", "failed"],
          ["Review", "failed"],
          ["Gone", "failed"],
        ]),
        api,
      );
      await submit(user);
      expect(await within(dialog()).findByText("1 of 4 destinations accepted.")).toBeTruthy();
      const list = within(within(dialog()).getByRole("list", { name: "Result per destination" }));
      const items = list.getAllByRole("listitem");
      expect(items).toHaveLength(4);
      const of = (label: string) =>
        within(items.find((item) => item.textContent?.startsWith(label)) as HTMLElement);
      expect(of("LIS").getByText("Accepted")).toBeTruthy();
      expect(of("Archive").getByText("Duplicate risk not confirmed")).toBeTruthy();
      expect(
        of("Archive").getByText(
          "Check the destination and confirm the risk of duplicate delivery.",
        ),
      ).toBeTruthy();
      expect(of("Review").getByText("Delivery in progress")).toBeTruthy();
      expect(of("Review").getByText("Wait for the existing delivery to finish.")).toBeTruthy();
      expect(of("Gone").getByText("Destination not available")).toBeTruthy();
      expect(
        of("Gone").getByText("Destination is absent or disabled in the active configuration."),
      ).toBeTruthy();
      expect(await screen.findByText("Resend accepted for 1 of 4 destinations")).toBeTruthy();
      await user.click(within(dialog()).getByRole("button", { name: "Back to the choices" }));
      expect(box("LIS").checked).toBe(true);
      expect(within(dialog()).getByRole("button", { name: "Resend" })).toBeTruthy();
      expect(onClose).not.toHaveBeenCalled();
    });

    it("says nothing was resent when every destination refused, and closes on Done", async () => {
      const api = stubApi({
        resendMessage: async () => ({
          newMessageId: null,
          destinations: [refusal.destinations[1] as ResendResult["destinations"][number]],
        }),
      });
      const { user, onClose } = open(subject([["Archive", "failed"]]), api);
      await submit(user);
      expect(await within(dialog()).findByText("0 of 1 destination accepted.")).toBeTruthy();
      expect(await screen.findByText("Nothing was resent")).toBeTruthy();
      await user.click(within(dialog()).getByRole("button", { name: "Done" }));
      expect(onClose).toHaveBeenCalled();
    });

    it("shows a refusal of the whole command in the engine's words, and stays open", async () => {
      const api = stubApi({
        resendMessage: async () => {
          throw engineError("not_found", "Message not found.");
        },
      });
      const { user, onClose } = open(subject([["LIS", "failed"]]), api);
      await submit(user);
      expect(await within(dialog()).findByText("The message was not resent")).toBeTruthy();
      expect(within(dialog()).getByText("Message not found.")).toBeTruthy();
      expect(within(dialog()).getByRole("button", { name: "Resend" })).toBeTruthy();
      expect(onClose).not.toHaveBeenCalled();
    });
  });

  it("closes on Cancel without asking the engine", async () => {
    const { user, api, onClose } = open(subject([["LIS", "failed"]]));
    await user.click(within(dialog()).getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalled();
    expect(api.resendMessage).not.toHaveBeenCalled();
  });

  describe("from the message", () => {
    const ROUTES = [{ path: "/messages/:id", element: <MessagePage /> }];
    const record = () =>
      detail({
        summary: summary(1, { deliverySummary: "attention" }),
        deliveries: [delivery("LIS", { state: "failed", ackLevel: "none" }), delivery("Archive")],
      });

    it("resends from the header with the failed delivery selected, and reads the message again", async () => {
      const api = stubApi({ getMessage: async () => record() });
      const { user } = renderRoutes(ROUTES, "/messages/msg-000001", { api });
      await screen.findByRole("heading", { level: 2, name: "Deliveries" });
      const reads = api.getMessage.mock.calls.length;
      await user.click(screen.getByRole("button", { name: "Resend…" }));
      expect(box("LIS").checked).toBe(true);
      expect(box("Archive").checked).toBe(false);
      await submit(user);
      expect(await within(dialog()).findByText("1 of 1 destination accepted.")).toBeTruthy();
      await waitFor(() => expect(api.getMessage.mock.calls.length).toBeGreaterThan(reads));
    });

    it("resends one destination from its own Resend… button", async () => {
      const api = stubApi({ getMessage: async () => record() });
      const { user } = renderRoutes(ROUTES, "/messages/msg-000001", { api });
      await screen.findByRole("heading", { level: 2, name: "Deliveries" });
      await user.click(screen.getByRole("button", { name: "Resend to Archive" }));
      expect(box("Archive").checked).toBe(true);
      expect(box("LIS").checked).toBe(false);
      // An acknowledged delivery may be at the destination already: the risk is asked for.
      expect(within(dialog()).getByRole("checkbox", { name: /twice/ })).toBeTruthy();
    });
  });
});
