import { screen, waitFor, within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import type { MessageQuery } from "@/api/types";
import { openRow } from "@/lib/test/grid";
import { mockGridLayout } from "@/lib/test/layout";
import { renderRoutes } from "@/lib/test/router";
import { chooseOption } from "@/lib/test/select";
import { detail, page, stubApi, summary } from "./fixtures";
import { ROUTES } from "./messagesListSetup";
import { forgetMessagesSession } from "./messagesSession";

const identifierBox = () =>
  screen.getByRole("searchbox", { name: "Identifier" }) as HTMLInputElement;
const textBox = () => screen.getByRole("searchbox", { name: "Message text" }) as HTMLInputElement;

/** The engine holds one message that matches `PAT-TEST-7`, and many that do not. */
function engine() {
  return stubApi({
    listMessages: async (query: MessageQuery) =>
      query.filter.identifier?.value === "PAT-TEST-7"
        ? page([summary(7)], 1)
        : page([summary(7), summary(8)], 13_700),
    getMessage: async (id: string) => detail({ summary: summary(Number(id.slice(-6))) }),
  });
}

/** Returns to the list by the breadcrumb on the message page. */
const backToList = (user: UserEvent) =>
  user.click(
    within(screen.getByRole("navigation", { name: "Breadcrumb" })).getByRole("link", {
      name: "Messages",
    }),
  );

const lastFilter = (api: ReturnType<typeof engine>) =>
  api.listMessages.mock.calls
    .map(([query]) => query)
    .filter((query) => query.limit > 1)
    .at(-1)?.filter;

describe("the Messages working set", () => {
  beforeEach(() => {
    mockGridLayout();
    forgetMessagesSession();
  });

  it("keeps the identifier and text from the list to a message and back by the breadcrumb", async () => {
    const api = engine();
    const { user, router } = renderRoutes(ROUTES, "/messages", { api });
    await screen.findByRole("grid");
    await user.type(identifierBox(), "PAT-TEST-7{Enter}");
    await user.type(textBox(), "glucose{Enter}");
    await waitFor(() => expect(screen.getByText("Rows 1–1 of 1")).toBeTruthy());
    await openRow(user, "CTRL-000007");
    expect(await screen.findByRole("heading", { level: 1, name: "CTRL-000007" })).toBeTruthy();
    await backToList(user);
    await screen.findByRole("grid");
    expect(identifierBox().value).toBe("PAT-TEST-7");
    expect(textBox().value).toBe("glucose");
    await waitFor(() => expect(screen.getByText("Rows 1–1 of 1")).toBeTruthy());
    expect(lastFilter(api)).toMatchObject({
      identifier: { value: "PAT-TEST-7", prefix: true },
      text: "glucose",
    });
    // Never in the address.
    expect(router.state.location.search).toBe("");
  });

  it("keeps them when the browser goes back", async () => {
    const api = engine();
    const { user, router } = renderRoutes(ROUTES, "/messages", { api });
    await screen.findByRole("grid");
    await user.type(identifierBox(), "PAT-TEST-7{Enter}");
    await openRow(user, "CTRL-000007");
    await screen.findByRole("heading", { level: 1, name: "CTRL-000007" });
    await router.navigate(-1);
    await screen.findByRole("grid");
    expect(identifierBox().value).toBe("PAT-TEST-7");
    await waitFor(() => expect(screen.getByText("Rows 1–1 of 1")).toBeTruthy());
  });

  it("keeps the exact-match choice and the message ID, and the rows per page", async () => {
    const api = engine();
    const { user } = renderRoutes(ROUTES, "/messages", { api });
    await screen.findByRole("grid");
    await user.click(screen.getByRole("button", { name: "More filters" }));
    await user.type(screen.getByRole("searchbox", { name: "Message ID" }), "0a1b2c3d{Enter}");
    await chooseOption(user, /Identifier match/, "Exactly");
    await user.type(identifierBox(), "PAT-TEST-7{Enter}");
    await chooseOption(user, /Rows per page/, "50");
    await openRow(user, "CTRL-000007");
    await screen.findByRole("heading", { level: 1, name: "CTRL-000007" });
    await backToList(user);
    await screen.findByRole("grid");
    await waitFor(() =>
      expect(lastFilter(api)).toMatchObject({
        identifier: { value: "PAT-TEST-7", prefix: false },
        messageIdPrefix: "0a1b2c3d",
      }),
    );
    expect(api.listMessages.mock.calls.at(-1)?.[0].limit).toBe(50);
  });

  it("forgets them when Clear filters is used, and a visit to a message and back finds none", async () => {
    const api = engine();
    const { user } = renderRoutes(ROUTES, "/messages", { api });
    await screen.findByRole("grid");
    await user.type(identifierBox(), "PAT-TEST-7{Enter}");
    await user.click(await screen.findByRole("button", { name: "Clear filters" }));
    await waitFor(() => expect(identifierBox().value).toBe(""));
    await openRow(user, "CTRL-000007");
    await screen.findByRole("heading", { level: 1, name: "CTRL-000007" });
    await backToList(user);
    await screen.findByRole("grid");
    expect(identifierBox().value).toBe("");
    await waitFor(() => expect(screen.getByText("Rows 1–2 of 13,700")).toBeTruthy());
  });

  it("is forgotten when the console is closed, and never written to storage", async () => {
    const api = engine();
    const first = renderRoutes(ROUTES, "/messages", { api });
    await screen.findByRole("grid");
    await first.user.type(identifierBox(), "PAT-TEST-7{Enter}");
    await waitFor(() => expect(screen.getByText("Rows 1–1 of 1")).toBeTruthy());
    expect(window.localStorage.length).toBe(0);
    expect(window.sessionStorage.length).toBe(0);
    first.unmount();
    // A reload starts the module again: nothing of the session is left.
    forgetMessagesSession();
    renderRoutes(ROUTES, "/messages", { api });
    await screen.findByRole("grid");
    expect(identifierBox().value).toBe("");
  });
});
