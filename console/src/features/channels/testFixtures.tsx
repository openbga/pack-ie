import type { LogEvent } from "@/api/types";
import { renderRoutes } from "@/lib/test/router";
import { ago, type channelDetail, snapshot, stubApi } from "../shared-operate/testFixtures";
import { ChannelPage } from "./ChannelPage";

export const channelRoutes = [{ path: "/channels/:id", element: <ChannelPage /> }];

/** A log line of `ch-alpha`, `seq * 10` seconds old. */
export const logEvent = (
  seq: number,
  message: string,
  level: LogEvent["level"] = "info",
): LogEvent => ({
  seq,
  at: ago(seq * 10),
  level,
  target: "packie_routing",
  event: null,
  channelId: "ch-alpha",
  endpointId: null,
  messageId: null,
  message,
  fields: {},
});

/** The channel page of `detail`, with the live snapshot, other API methods and snapshot fields overridable. */
export function renderChannel(
  detail: ReturnType<typeof channelDetail>,
  api: Record<string, unknown> = {},
  live = detail.snapshot,
  extra: Partial<ReturnType<typeof snapshot>> = {},
) {
  return renderRoutes(channelRoutes, `/channels/${detail.snapshot.id}`, {
    api: stubApi({
      getChannel: async () => detail,
      getSnapshot: async () => snapshot({ channels: [live], ...extra }),
      queryLogs: async () => ({ events: [], olderCursor: null }),
      ...api,
    }),
  });
}
