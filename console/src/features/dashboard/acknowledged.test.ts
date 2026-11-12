import { describe, expect, it } from "vitest";
import { ago, messageChannel } from "../shared-operate/testFixtures";
import { acknowledgedCounts, acknowledgedTimes } from "./acknowledged";

const dest = (label: string, acknowledged: number, enabled = true, at: string | null = null) => ({
  destinationId: label,
  label,
  enabled,
  acknowledged,
  lastAcknowledgedAt: at,
});
const NOW = Date.now();

describe("acknowledgedCounts", () => {
  it("is plain for one destination and per destination for several", () => {
    expect(acknowledgedCounts(messageChannel({ destinations: [dest("LIS", 1200)] }))).toBe(
      "Acknowledged 1,200",
    );
    expect(
      acknowledgedCounts(
        messageChannel({ destinations: [dest("LIS", 2136), dest("Backup", 1068)] }),
      ),
    ).toBe("Acknowledged 2,136 (LIS) · 1,068 (Backup)");
  });

  it("leaves out a disabled destination that acknowledged nothing, and says zero without any", () => {
    expect(
      acknowledgedCounts(messageChannel({ destinations: [dest("LIS", 5), dest("Off", 0, false)] })),
    ).toBe("Acknowledged 5");
    expect(acknowledgedCounts(messageChannel({ destinations: [dest("Off", 0, false)] }))).toBe(
      "Acknowledged 0",
    );
  });

  it("is null when the snapshot does not list destinations", () => {
    expect(acknowledgedCounts(messageChannel())).toBeNull();
  });
});

describe("acknowledgedTimes", () => {
  it("says when, per destination when there are several", () => {
    const at = ago(30);
    expect(
      acknowledgedTimes(messageChannel({ destinations: [dest("LIS", 1, true, at)] }), NOW),
    ).toBe("Acknowledged 30 s ago");
    expect(
      acknowledgedTimes(
        messageChannel({ destinations: [dest("LIS", 1, true, at), dest("Backup", 0)] }),
        NOW,
      ),
    ).toBe("Acknowledged 30 s ago (LIS) · never (Backup)");
    expect(acknowledgedTimes(messageChannel({ destinations: [dest("LIS", 0)] }), NOW)).toBe(
      "Acknowledged never",
    );
    expect(acknowledgedTimes(messageChannel(), NOW)).toBeNull();
  });
});
