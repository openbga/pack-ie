import type { ComponentType } from "react";
import { Navigate, type RouteObject } from "react-router";
import { AriaRouter } from "@/lib/AriaRouter";
import { AppShell } from "./AppShell";
import { HOME_PATH } from "./navigation";

/**
 * A route whose screen module loads on first navigation. The router waits for it before it
 * changes the location, so the screen arrives with its heading in place.
 */
function lazyRoute(path: string, page: () => Promise<ComponentType>): RouteObject {
  return { path, lazy: async () => ({ Component: await page() }) };
}

/** docs/architecture.md §8. List → detail pages are separate routes; edit pages sit under them. */
export const routes: RouteObject[] = [
  {
    element: <AriaRouter />,
    children: [
      {
        path: "/",
        element: <AppShell />,
        // Nothing to show while the first screen's module loads; it is local and takes milliseconds.
        hydrateFallbackElement: null,
        children: [
          { index: true, element: <Navigate to={HOME_PATH} replace /> },
          lazyRoute("/dashboard", () =>
            import("@/features/dashboard/DashboardPage").then((m) => m.DashboardPage),
          ),
          lazyRoute("/channels", () =>
            import("@/features/channels/ChannelsPage").then((m) => m.ChannelsPage),
          ),
          lazyRoute("/channels/new", () =>
            import("@/features/config/ChannelEditPage").then((m) => m.ChannelEditPage),
          ),
          lazyRoute("/channels/:id", () =>
            import("@/features/channels/ChannelPage").then((m) => m.ChannelPage),
          ),
          lazyRoute("/channels/:id/edit", () =>
            import("@/features/config/ChannelEditPage").then((m) => m.ChannelEditPage),
          ),
          lazyRoute("/endpoints", () =>
            import("@/features/endpoints/EndpointsPage").then((m) => m.EndpointsPage),
          ),
          lazyRoute("/endpoints/new", () =>
            import("@/features/config/EndpointEditPage").then((m) => m.EndpointEditPage),
          ),
          lazyRoute("/endpoints/:id", () =>
            import("@/features/endpoints/EndpointPage").then((m) => m.EndpointPage),
          ),
          lazyRoute("/endpoints/:id/edit", () =>
            import("@/features/config/EndpointEditPage").then((m) => m.EndpointEditPage),
          ),
          lazyRoute("/messages", () =>
            import("@/features/messages/MessagesPage").then((m) => m.MessagesPage),
          ),
          lazyRoute("/messages/:id", () =>
            import("@/features/messages/MessagePage").then((m) => m.MessagePage),
          ),
          lazyRoute("/logs", () => import("@/features/logs/LogsPage").then((m) => m.LogsPage)),
          lazyRoute("/alerts", () =>
            import("@/features/alerts/AlertsPage").then((m) => m.AlertsPage),
          ),
          lazyRoute("/audit", () => import("@/features/audit/AuditPage").then((m) => m.AuditPage)),
          lazyRoute("/transmission", () =>
            import("@/features/transmission/TransmissionPage").then((m) => m.TransmissionPage),
          ),
          { path: "/settings", element: <Navigate to="/settings/engine" replace /> },
          lazyRoute("/settings/:tab", () =>
            import("@/features/settings/SettingsPage").then((m) => m.SettingsPage),
          ),
        ],
      },
    ],
  },
  { path: "*", element: <Navigate to="/" replace /> },
];
