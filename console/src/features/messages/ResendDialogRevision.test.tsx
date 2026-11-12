import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { engineError } from "@/lib/test/engineError";
import { renderRoutes } from "@/lib/test/router";
import { delivery, detail, stubApi, summary } from "./fixtures";
import { MessagePage } from "./MessagePage";
import { accepted, dialog, open, subject, submit } from "./resendDialogSetup";

describe("ResendDialog", () => {
  describe("a message that a newer revision supersedes", () => {
    const NEWER = "msg-000009";
    const supersededRecord = (revisedBy: string | null) => detail({ revisedBy });
    const newerRecord = () =>
      detail({ summary: summary(9, { receivedAt: "2026-11-06T13:15:30Z" }) });
    const reads = (revisedBy: string | null) => async (id: string) =>
      id === NEWER ? newerRecord() : supersededRecord(revisedBy);
    const refusal = () => ({
      ...engineError(
        "conflict",
        `A newer revision of this message exists (${NEWER}, received 2026-11-06T13:15:30Z). Resending this older version may reissue a corrected result; confirm that it is intended to resend it anyway.`,
      ),
      rejection: "superseded" as const,
    });
    const confirmation = () =>
      within(dialog()).getByRole("checkbox", {
        name: "I understand this may reissue a corrected result; resend this older version anyway",
      }) as HTMLInputElement;

    it("warns with the newer message linked and its time, and holds Resend until confirmed", async () => {
      const api = stubApi({ getMessage: reads(NEWER) });
      const { user, container } = open(subject([["LIS", "failed"]]), api);
      expect(
        await within(dialog()).findByText("A newer revision of this message exists"),
      ).toBeTruthy();
      const link = await within(dialog()).findByRole("link", { name: NEWER });
      expect(link.getAttribute("href")).toBe(`/messages/${NEWER}`);
      expect(within(dialog()).getByText(/A newer revision exists, received/)).toBeTruthy();
      expect(await within(dialog()).findByText(/2026-11-06 .*13:15:30/)).toBeTruthy();
      expect(within(dialog()).getByText(/may reissue a corrected result\./)).toBeTruthy();
      const resend = within(dialog()).getByRole("button", { name: "Resend" });
      expect(resend.hasAttribute("disabled")).toBe(true);
      expect(confirmation().checked).toBe(false);
      await expectNoA11yViolations(container);

      await user.click(confirmation());
      expect(resend.hasAttribute("disabled")).toBe(false);
      await user.click(resend);
      await waitFor(() => expect(api.resendMessage).toHaveBeenCalled());
      expect(api.resendMessage).toHaveBeenCalledWith({
        messageId: "msg-000001",
        destinations: ["dest-LIS"],
        mode: "same_message",
        confirmDuplicateRisk: false,
        confirmSuperseded: true,
      });
      expect(await within(dialog()).findByText("1 of 1 destination accepted.")).toBeTruthy();
    });

    it("holds Resend again when the confirmation is withdrawn", async () => {
      const { user } = open(subject([["LIS", "failed"]]), stubApi({ getMessage: reads(NEWER) }));
      await user.click(
        await within(dialog()).findByRole("checkbox", { name: /resend this older version anyway/ }),
      );
      await user.click(confirmation());
      expect(
        within(dialog()).getByRole("button", { name: "Resend" }).hasAttribute("disabled"),
      ).toBe(true);
    });

    it("asks nothing of a message nothing supersedes", async () => {
      const api = stubApi({ getMessage: reads(null) });
      const { user } = open(subject([["LIS", "failed"]]), api);
      await waitFor(() => expect(api.getMessage).toHaveBeenCalled());
      expect(within(dialog()).queryByText("A newer revision of this message exists")).toBeNull();
      expect(
        within(dialog()).getByRole("button", { name: "Resend" }).hasAttribute("disabled"),
      ).toBe(false);
      await submit(user);
      await waitFor(() => expect(api.resendMessage).toHaveBeenCalled());
      const request = api.resendMessage.mock.calls[0]?.[0];
      expect(request && "confirmSuperseded" in request).toBe(false);
    });

    it("shows the engine's refusal when a revision arrived meanwhile, links it, and resends once confirmed", async () => {
      let arrived = false;
      const api = stubApi({
        getMessage: async (id: string) =>
          id === NEWER ? newerRecord() : supersededRecord(arrived ? NEWER : null),
        resendMessage: vi
          .fn()
          .mockImplementationOnce(async () => {
            arrived = true;
            throw refusal();
          })
          .mockImplementation(async () => ({
            newMessageId: null,
            destinations: [accepted("dest-LIS", "LIS")],
          })),
      });
      const { user } = open(subject([["LIS", "failed"]]), api);
      await submit(user);
      expect(await within(dialog()).findByText("The message was not resent")).toBeTruthy();
      expect(
        within(dialog()).getByText(
          /A newer revision of this message exists \(msg-000009, received 2026-11-06T13:15:30Z\)/,
        ),
      ).toBeTruthy();
      const link = await within(dialog()).findByRole("link", { name: NEWER });
      expect(link.getAttribute("href")).toBe(`/messages/${NEWER}`);
      expect(confirmation().checked).toBe(false);
      const resend = within(dialog()).getByRole("button", { name: "Resend" });
      expect(resend.hasAttribute("disabled")).toBe(true);
      await user.click(confirmation());
      await user.click(resend);
      expect(await within(dialog()).findByText("1 of 1 destination accepted.")).toBeTruthy();
      expect(api.resendMessage).toHaveBeenLastCalledWith(
        expect.objectContaining({ confirmSuperseded: true }),
      );
    });

    it("still asks for the confirmation when the engine refuses and the record cannot be read", async () => {
      const api = stubApi({
        getMessage: async () => {
          throw engineError("internal", "Could not read the message.");
        },
        resendMessage: async () => {
          throw refusal();
        },
      });
      const { user } = open(subject([["LIS", "failed"]]), api);
      await submit(user);
      expect(await within(dialog()).findByText("The message was not resent")).toBeTruthy();
      expect(
        await within(dialog()).findByText("A newer revision of this message exists"),
      ).toBeTruthy();
      expect(within(dialog()).queryByRole("link", { name: NEWER })).toBeNull();
      expect(
        within(dialog()).getByRole("button", { name: "Resend" }).hasAttribute("disabled"),
      ).toBe(true);
    });

    it("is learned from the message page the dialog opens on, which shows the notice and link", async () => {
      const api = stubApi({
        getMessage: async (id: string) =>
          id === NEWER
            ? newerRecord()
            : detail({
                summary: summary(1, { deliverySummary: "attention" }),
                deliveries: [delivery("LIS", { state: "failed", ackLevel: "none" })],
                revisedBy: NEWER,
              }),
      });
      const { user } = renderRoutes(
        [{ path: "/messages/:id", element: <MessagePage /> }],
        "/messages/msg-000001",
        { api },
      );
      expect(await screen.findByText("Superseded by a newer revision")).toBeTruthy();
      expect(
        screen.getByRole("link", { name: "Open the newer message" }).getAttribute("href"),
      ).toBe(`/messages/${NEWER}`);
      expect(screen.getByRole("link", { name: "The newest revision" }).getAttribute("href")).toBe(
        `/messages/${NEWER}`,
      );
      await user.click(screen.getByRole("button", { name: "Resend…" }));
      expect(await within(dialog()).findByRole("link", { name: NEWER })).toBeTruthy();
      expect(
        within(dialog()).getByRole("button", { name: "Resend" }).hasAttribute("disabled"),
      ).toBe(true);
    });
  });
});
