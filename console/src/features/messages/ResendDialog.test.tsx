import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { stubApi } from "./fixtures";
import { accepted, box, dialog, open, subject, submit } from "./resendDialogSetup";

describe("ResendDialog", () => {
  it("starts with the deliveries that need attention, and the same-message mode", async () => {
    const { container } = open(
      subject([
        ["LIS", "failed"],
        ["Archive", "acknowledged"],
        ["Review", "rejected"],
      ]),
    );
    expect(box("LIS").checked).toBe(true);
    expect(box("Review").checked).toBe(true);
    expect(box("Archive").checked).toBe(false);
    expect(within(dialog()).getByText("Failed, not delivered")).toBeTruthy();
    expect(within(dialog()).getByText("Acknowledged")).toBeTruthy();
    expect(
      (within(dialog()).getByRole("radio", { name: /Same message/ }) as HTMLInputElement).checked,
    ).toBe(true);
    expect(within(dialog()).getByText("Try again with the same output")).toBeTruthy();
    expect(
      within(dialog()).getByText("Process the original again with the current configuration"),
    ).toBeTruthy();
    expect(within(dialog()).queryByRole("checkbox", { name: /twice/ })).toBeNull();
    await expectNoA11yViolations(container);
  });

  it("resends the chosen destinations in the chosen mode and lists what happened", async () => {
    const api = stubApi({
      resendMessage: async () => ({
        newMessageId: null,
        destinations: [accepted("dest-LIS", "LIS")],
      }),
    });
    const { user } = open(
      subject([
        ["LIS", "failed"],
        ["Archive", "acknowledged"],
      ]),
      api,
    );
    await submit(user);
    expect(api.resendMessage).toHaveBeenCalledWith({
      messageId: "msg-000001",
      destinations: ["dest-LIS"],
      mode: "same_message",
      confirmDuplicateRisk: false,
    });
    expect(await within(dialog()).findByText("1 of 1 destination accepted.")).toBeTruthy();
    const item = within(within(dialog()).getByRole("list", { name: "Result per destination" }));
    expect(item.getByText("LIS")).toBeTruthy();
    expect(item.getByText("Accepted")).toBeTruthy();
    expect(item.getByText(/Queued again/)).toBeTruthy();
    expect(await screen.findByText("Resend accepted for 1 destination")).toBeTruthy();
  });

  describe("the reason", () => {
    const failed = () => subject([["LIS", "failed"]]);
    const reasonBox = () =>
      within(dialog()).getByRole("textbox", { name: /Reason/ }) as HTMLTextAreaElement;

    it("is optional, says who sees it, and counts characters", async () => {
      const { user, container } = open(failed());
      expect(reasonBox().required).toBe(false);
      expect(
        within(dialog()).getByText(
          /Recorded with the new attempt and visible to everyone who opens this message\. 0 of 500 characters\./,
        ),
      ).toBeTruthy();
      await user.type(reasonBox(), "Destination was down");
      expect(within(dialog()).getByText(/20 of 500 characters/)).toBeTruthy();
      await expectNoA11yViolations(container);
    });

    it("is sent trimmed with the resend", async () => {
      const { user, api } = open(failed());
      await user.type(reasonBox(), "  Destination was down  ");
      await submit(user);
      expect(api.resendMessage).toHaveBeenCalledWith({
        messageId: "msg-000001",
        destinations: ["dest-LIS"],
        mode: "same_message",
        confirmDuplicateRisk: false,
        reason: "Destination was down",
      });
    });

    it("is left out when empty or only spaces", async () => {
      const { user, api } = open(failed());
      await user.type(reasonBox(), "   ");
      await submit(user);
      const request = api.resendMessage.mock.calls[0]?.[0];
      expect(request).toBeDefined();
      expect(request && "reason" in request).toBe(false);
    });

    it("stops at 500 characters with a message and sends nothing", async () => {
      const { user, api } = open(failed());
      await user.click(reasonBox());
      await user.paste("x".repeat(501));
      await submit(user);
      expect(await within(dialog()).findByText("Use 500 characters or fewer.")).toBeTruthy();
      expect(api.resendMessage).not.toHaveBeenCalled();
    });
  });

  it("needs at least one destination", async () => {
    const { user, api } = open(subject([["LIS", "acknowledged"]]));
    await submit(user);
    expect(await within(dialog()).findByText("Choose at least one destination.")).toBeTruthy();
    expect(api.resendMessage).not.toHaveBeenCalled();
  });

  describe("the duplicate-risk confirmation", () => {
    it.each([
      ["queued", false],
      ["failed", false],
      ["rejected", false],
      ["cancelled", false],
      ["acknowledged", true],
      ["timed_out", true],
      ["outcome_unknown", true],
    ] as const)(
      "is asked for a selected %s delivery exactly when it applies: %s",
      async (state, asked) => {
        const { user } = open(subject([["LIS", state]]), stubApi(), vi.fn(), ["dest-LIS"]);
        expect(within(dialog()).queryByRole("checkbox", { name: /twice/ }) !== null).toBe(asked);
        expect(within(dialog()).queryByText(/may already have this message/) !== null).toBe(asked);
        // Choosing the new message mode does not change what the destination may have processed.
        await user.click(within(dialog()).getByRole("radio", { name: /New message/ }));
        expect(within(dialog()).queryByRole("checkbox", { name: /twice/ }) !== null).toBe(asked);
      },
    );

    it("appears and goes with the selection, and blocks the resend until it is given", async () => {
      const { user, api } = open(
        subject([
          ["LIS", "failed"],
          ["Archive", "acknowledged"],
        ]),
      );
      expect(within(dialog()).queryByRole("checkbox", { name: /twice/ })).toBeNull();
      await user.click(box("Archive"));
      expect(within(dialog()).getByRole("checkbox", { name: /twice/ })).toBeTruthy();
      await submit(user);
      expect(
        await within(dialog()).findByText("Confirm the duplicate risk to resend these deliveries."),
      ).toBeTruthy();
      expect(api.resendMessage).not.toHaveBeenCalled();
      await user.click(box(/twice/));
      await submit(user);
      await waitFor(() =>
        expect(api.resendMessage).toHaveBeenCalledWith({
          messageId: "msg-000001",
          destinations: ["dest-LIS", "dest-Archive"],
          mode: "same_message",
          confirmDuplicateRisk: true,
        }),
      );
    });

    it("is not sent when it was given and then no longer applies", async () => {
      const { user, api } = open(
        subject([
          ["LIS", "failed"],
          ["Archive", "acknowledged"],
        ]),
      );
      await user.click(box("Archive"));
      await user.click(box(/twice/));
      await user.click(box("Archive"));
      await submit(user);
      await waitFor(() => expect(api.resendMessage).toHaveBeenCalled());
      expect(api.resendMessage.mock.calls[0]?.[0]).toMatchObject({
        destinations: ["dest-LIS"],
        confirmDuplicateRisk: false,
      });
    });
  });

  describe("modes", () => {
    it("does not take a delivery in progress for the same message, but takes it for a new message", async () => {
      const { user, api } = open(
        subject([
          ["LIS", "queued"],
          ["Archive", "failed"],
        ]),
        stubApi(),
        vi.fn(),
        ["dest-LIS", "dest-Archive"],
      );
      await submit(user);
      expect(
        await within(dialog()).findByText(/cannot be resent as the same message/),
      ).toBeTruthy();
      expect(api.resendMessage).not.toHaveBeenCalled();
      await user.click(within(dialog()).getByRole("radio", { name: /New message/ }));
      await submit(user, "Create new message");
      await waitFor(() => expect(api.resendMessage).toHaveBeenCalled());
      expect(api.resendMessage.mock.calls[0]?.[0]).toMatchObject({
        mode: "new_message",
        destinations: ["dest-LIS", "dest-Archive"],
      });
    });

    it.each(["invalid", "aborted", "filtered", "duplicate"] as const)(
      "offers only a new message for a %s message, for every destination",
      async (state) => {
        const api = stubApi({
          resendMessage: async () => ({
            newMessageId: "msg-new-1",
            destinations: [accepted("dest-LIS", "LIS", false)],
          }),
        });
        const { user } = open(subject([], state), api);
        expect(within(dialog()).queryByRole("radio")).toBeNull();
        expect(within(dialog()).queryByRole("checkbox")).toBeNull();
        expect(
          within(dialog()).getByText(/can only be processed again as a new message/),
        ).toBeTruthy();
        expect(within(dialog()).getByText(/This message has no deliveries/)).toBeTruthy();
        await submit(user, "Create new message");
        await waitFor(() =>
          expect(api.resendMessage).toHaveBeenCalledWith({
            messageId: "msg-000001",
            destinations: "all",
            mode: "new_message",
            confirmDuplicateRisk: false,
          }),
        );
        expect(
          await within(dialog()).findByRole("link", { name: "Open the new message" }),
        ).toBeTruthy();
        expect(
          within(dialog()).getByRole("link", { name: "Open the new message" }).getAttribute("href"),
        ).toBe("/messages/msg-new-1");
        expect(within(dialog()).getByText(/Accepted\. The new message is processed/)).toBeTruthy();
      },
    );

    it("offers only a new message for a routed message that has no deliveries, too", () => {
      open(subject([], "routed"));
      expect(within(dialog()).queryByRole("radio")).toBeNull();
      expect(within(dialog()).getByText(/No delivery exists to try again/)).toBeTruthy();
    });
  });
});
