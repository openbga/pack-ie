import { screen, within } from "@testing-library/react";
import type { MessageState } from "@/api/types";
import { renderRoutes } from "@/lib/test/router";
import { detail, type StubApi, summary } from "./fixtures";
import { MessagePage } from "./MessagePage";

export const ROUTES = [{ path: "/messages/:id", element: <MessagePage /> }];
export const PATH = "/messages/msg-000001";

/** Renders the message page for `path` and waits for the record to load. */
export async function findRecord(api: StubApi, path = PATH) {
  const rendered = renderRoutes(ROUTES, path, { api });
  await screen.findByRole("heading", { level: 2, name: "Deliveries" });
  return rendered;
}

export const deliveryCard = (label: string) => within(screen.getByRole("article", { name: label }));

/** A record for a message that was not routed: no deliveries. */
export const nonRouted = (state: MessageState, extra: Parameters<typeof detail>[0] = {}) =>
  detail({
    summary: summary(1, { state, deliverySummary: "none", deliveries: [] }),
    deliveries: [],
    ...extra,
  });
