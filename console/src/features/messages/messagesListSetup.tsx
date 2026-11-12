import type { MessageQuery } from "@/api/types";
import type { StubApi } from "./fixtures";
import { MessagePage } from "./MessagePage";
import { MessagesPage } from "./MessagesPage";

export const ROUTES = [
  { path: "/messages", element: <MessagesPage /> },
  { path: "/messages/:id", element: <MessagePage /> },
];

/** The queries for pages of rows; the quick filters' counts ask for one row. */
export const listQueries = (api: StubApi) =>
  api.listMessages.mock.calls.map(([query]) => query).filter((query) => query.limit > 1);
export const lastQuery = (api: StubApi): MessageQuery | undefined => listQueries(api).at(-1);
