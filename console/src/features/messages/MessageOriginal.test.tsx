import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { engineError } from "@/lib/test/engineError";
import { fieldsOf, HL7_TEXT, messageBody, stubApi } from "./fixtures";
import { findRecord, PATH } from "./messageRecordSetup";

describe("MessagePage", () => {
  describe("the original message", () => {
    it("is not read until it is asked for, and never put in the address or the title", async () => {
      const api = stubApi();
      const { user, router } = await findRecord(api);
      expect(api.getMessageBody).not.toHaveBeenCalled();
      expect(
        screen.getAllByText(/It is not kept in the address or the window title/).length,
      ).toBeGreaterThan(0);
      await user.click(screen.getByRole("button", { name: "Show original message" }));
      expect(api.getMessageBody).toHaveBeenCalledWith({
        messageId: "msg-000001",
        which: { kind: "original" },
        as: "text",
      });
      const region = await screen.findByRole("region", { name: "Original message as text" });
      expect(region.textContent).toContain("<STX>MSH|^~\\&|SYN|LAB");
      expect(region.textContent).toContain("<CR>");
      expect(router.state.location.search).toBe("");
      expect(document.title).not.toContain("MSH");
      expect(router.state.location.pathname).toBe(PATH);
    });

    it("shows the hex dump the engine renders when the toggle is pressed, and the text again", async () => {
      const api = stubApi({
        getMessageBody: async (request: { as: string }) =>
          messageBody(
            request.as === "hex"
              ? { as: "hex", content: "00000000  02 4d 53 48  .MSH\n" }
              : { as: "text" },
          ),
      });
      const { user } = await findRecord(api);
      await user.click(screen.getByRole("button", { name: "Show original message" }));
      await screen.findByRole("region", { name: "Original message as text" });
      await user.click(screen.getByRole("radio", { name: "Hex" }));
      const hex = await screen.findByRole("region", { name: "Original message as hex" });
      expect(hex.textContent).toBe("00000000  02 4d 53 48  .MSH\n");
      expect(api.getMessageBody).toHaveBeenLastCalledWith({
        messageId: "msg-000001",
        which: { kind: "original" },
        as: "hex",
      });
      const calls = api.getMessageBody.mock.calls.length;
      await user.click(screen.getByRole("radio", { name: "Text" }));
      expect(await screen.findByRole("region", { name: "Original message as text" })).toBeTruthy();
      expect(api.getMessageBody.mock.calls.length).toBe(calls);
    });

    describe("the fields view", () => {
      /** Opens the original, picks Fields and asks for the content, as an operator must. */
      async function showFields(user: Awaited<ReturnType<typeof findRecord>>["user"]) {
        await user.click(screen.getByRole("button", { name: "Show original message" }));
        await user.click(await screen.findByRole("radio", { name: "Fields" }));
        await user.click(await screen.findByRole("button", { name: "Show content" }));
      }

      it("reads the engine's fields only on an explicit Show content, which the engine audits", async () => {
        const api = stubApi();
        const { user } = await findRecord(api);
        expect(screen.queryByText("MSH.9.1")).toBeNull();
        expect(screen.queryByText("MRN-00000")).toBeNull();
        await user.click(screen.getByRole("button", { name: "Show original message" }));
        await screen.findByRole("region", { name: "Original message as text" });
        await user.click(screen.getByRole("radio", { name: "Fields" }));
        // Choosing the view reads nothing: the fields are message content.
        expect(api.getMessageFields).not.toHaveBeenCalled();
        expect(screen.getByText(/records this in the audit log/)).toBeTruthy();
        await user.click(screen.getByRole("button", { name: "Show content" }));
        const region = await screen.findByRole("region", { name: "Original message as fields" });
        expect(api.getMessageFields).toHaveBeenCalledTimes(1);
        expect(api.getMessageFields).toHaveBeenCalledWith("msg-000001");
        expect(screen.getByText("Read by the engine from the original message.")).toBeTruthy();
        const table = within(region);
        expect(
          table.getByRole("rowheader", { name: "MSH.9.1" }).nextElementSibling?.textContent,
        ).toBe("ORU");
        expect(
          table.getByRole("rowheader", { name: "OBX[1].5" }).nextElementSibling?.textContent,
        ).toBe("140");
        expect(table.getByRole("columnheader", { name: "Path" })).toBeTruthy();
        expect(screen.queryByText("More fields exist than are listed.")).toBeNull();
        // Hiding the original takes them off the page again, and asking again reads again.
        await user.click(screen.getByRole("button", { name: "Hide" }));
        expect(screen.queryByText("MSH.9.1")).toBeNull();
        expect(screen.queryByText("MRN-00000")).toBeNull();
      });

      it("finds a field by its path or its value, and says when none match", async () => {
        const { user } = await findRecord(stubApi());
        await showFields(user);
        const find = await screen.findByRole("searchbox", {
          name: "Find in original message fields",
        });
        await user.type(find, "obx");
        const region = await screen.findByRole("region", { name: "Original message as fields" });
        await waitFor(() => expect(within(region).queryByText("MSH.9.1")).toBeNull());
        expect(within(region).getByText("OBX.5")).toBeTruthy();
        expect(screen.getByText("2 of 6 fields")).toBeTruthy();
        await user.clear(find);
        await user.type(find, "CTRL-0");
        await waitFor(() => expect(within(region).queryByText("OBX.5")).toBeNull());
        expect(within(region).getByText("MSH.10")).toBeTruthy();
        await user.clear(find);
        await user.type(find, "nothing like this");
        expect(await screen.findByText(/No field has a path or value containing/)).toBeTruthy();
      });

      it("says that more fields exist than are listed", async () => {
        const api = stubApi({ getMessageFields: async () => fieldsOf({ truncated: true }) });
        const { user } = await findRecord(api);
        await showFields(user);
        expect(await screen.findByText("More fields exist than are listed.")).toBeTruthy();
      });

      it("gives the engine's reason when it lists no fields", async () => {
        const reason = "Fields are listed for HL7 and ASTM messages.";
        const api = stubApi({
          getMessageFields: async () => fieldsOf({ fields: [], unavailable: reason }),
        });
        const { user } = await findRecord(api);
        await showFields(user);
        expect(await screen.findByText(reason)).toBeTruthy();
        expect(screen.queryByRole("region", { name: "Original message as fields" })).toBeNull();
        expect(screen.queryByRole("searchbox")).toBeNull();
      });

      it("says so when the engine lists none and gives no reason", async () => {
        const api = stubApi({ getMessageFields: async () => fieldsOf({ fields: [] }) });
        const { user } = await findRecord(api);
        await showFields(user);
        expect(
          await screen.findByText("The engine listed no fields for this message."),
        ).toBeTruthy();
      });

      it("says why when the fields cannot be read, such as an audit log that cannot be written", async () => {
        const api = stubApi({
          getMessageFields: async () => {
            throw engineError("unavailable", "The audit log cannot be written.");
          },
        });
        const { user } = await findRecord(api);
        await showFields(user);
        expect(await screen.findByText("The audit log cannot be written.")).toBeTruthy();
        expect(screen.queryByText("MSH.9.1")).toBeNull();
      });

      it("copies the text while the fields are shown, and has no violations", async () => {
        const { user, container } = await findRecord(stubApi());
        await showFields(user);
        await screen.findByRole("region", { name: "Original message as fields" });
        await user.click(screen.getByRole("button", { name: "Copy as text" }));
        expect(await navigator.clipboard.readText()).toBe(HL7_TEXT);
        await expectNoA11yViolations(container, { wholePage: false });
      });

      it("is offered for the original message only, not for a transformed one", async () => {
        const { user } = await findRecord(stubApi());
        await user.click(screen.getByRole("button", { name: "Show original message" }));
        await screen.findByRole("radio", { name: "Fields" });
        await user.click(
          screen.getAllByRole("button", { name: /Show transformed message/ })[0] as HTMLElement,
        );
        const groups = await screen.findAllByRole("radiogroup");
        expect(groups.map((group) => within(group).queryAllByRole("radio").length)).toEqual([3, 2]);
      });
    });

    it("says when the engine cut the message short, and when it is empty", async () => {
      const api = stubApi({
        getMessageBody: async () => messageBody({ truncated: true, byteLength: 300_000 }),
      });
      const { user } = await findRecord(api);
      await user.click(screen.getByRole("button", { name: "Show original message" }));
      expect(await screen.findByText("This message is cut short")).toBeTruthy();
      expect(screen.getByText(/more than the engine shows/)).toBeTruthy();
    });

    it("shows an empty message as empty", async () => {
      const api = stubApi({
        getMessageBody: async () => messageBody({ content: "", byteLength: 0 }),
      });
      const { user } = await findRecord(api);
      await user.click(screen.getByRole("button", { name: "Show original message" }));
      expect(await screen.findByText("This message is empty.")).toBeTruthy();
    });

    it("copies what is shown, and hides it again", async () => {
      const { user } = await findRecord(stubApi());
      await user.click(screen.getByRole("button", { name: "Show original message" }));
      await screen.findByRole("region", { name: "Original message as text" });
      await user.click(screen.getByRole("button", { name: "Copy as text" }));
      expect(await navigator.clipboard.readText()).toBe(HL7_TEXT);
      expect(await screen.findByText("Original message copied as text")).toBeTruthy();
      await user.click(screen.getByRole("button", { name: "Hide" }));
      expect(screen.queryByRole("region", { name: "Original message as text" })).toBeNull();
      expect(screen.getByRole("button", { name: "Show original message" })).toBeTruthy();
    });

    it("shows the engine's refusal with a retry, and not the message", async () => {
      let fail = true;
      const api = stubApi({
        getMessageBody: async () => {
          if (fail) throw engineError("not_found", "The body was purged.");
          return messageBody();
        },
      });
      const { user } = await findRecord(api);
      await user.click(screen.getByRole("button", { name: "Show original message" }));
      expect(await screen.findByText("The original message could not be loaded")).toBeTruthy();
      expect(screen.getByText("The body was purged.")).toBeTruthy();
      fail = false;
      await user.click(screen.getByRole("button", { name: "Try again" }));
      expect(await screen.findByRole("region", { name: "Original message as text" })).toBeTruthy();
    });
  });
});
