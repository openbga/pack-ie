import { describe, expect, it } from "vitest";
import { stopReasonText } from "@/features/channels/channelLabels";
import { channelShape } from "./stateShapes";

describe("a channel waiting for the message store", () => {
  const channel = { state: "stopped", stopReason: "store_unavailable" } as const;

  it("is named in words, with the neutral stopped shape", () => {
    expect(channelShape(channel)).toMatchObject({
      tone: "neutral",
      label: "Waiting for the message store",
    });
  });

  it("says the engine keeps trying and starts the channel when the store opens", () => {
    expect(stopReasonText("store_unavailable")).toMatch(
      /^Waiting for the message store.*starts the channel when it opens/,
    );
  });
});
