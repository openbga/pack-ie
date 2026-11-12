import { describe, expect, it } from "vitest";
import type { MessageExportRequest } from "../types";
import { exportToIpc } from "./reports";

describe("exportToIpc", () => {
  it("sends the list's filter as the engine reads it, the format and the content choice", () => {
    const request: MessageExportRequest = {
      filter: {
        since: "2026-11-06T00:00:00Z",
        channelIds: ["ch-1"],
        states: ["routed"],
        identifier: { value: "PT-1", prefix: true },
      },
      format: "jsonl",
      includeBodies: false,
    };
    expect(exportToIpc(request)).toEqual({
      filter: expect.objectContaining({
        since: "2026-11-06T00:00:00Z",
        channels: ["ch-1"],
        states: ["routed"],
        identifier: { value: "PT-1", prefix: true },
      }),
      format: "jsonl",
      include_bodies: false,
    });
  });

  it("asks for the bodies only when the request does, and never sets an offset itself", () => {
    const sent = exportToIpc({ filter: {}, format: "csv", includeBodies: true });
    expect(sent).toMatchObject({ format: "csv", include_bodies: true });
    expect("offset" in sent).toBe(false);
  });
});
