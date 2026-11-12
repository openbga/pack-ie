import { describe, expect, it } from "vitest";
import type { TrafficSegment } from "@/api/types";
import { decodeBase64, flowMarker, toHex, toSymbolicText } from "./trafficText";

const bytes = (...values: number[]) => new Uint8Array(values);

describe("traffic text", () => {
  it("names control characters and keeps printable ASCII as is", () => {
    const frame = bytes(0x02, 0x31, 0x48, 0x7c, 0x0d, 0x03, 0x0d, 0x0a);
    expect(toSymbolicText(frame)).toBe("<STX>1H|<CR><ETX><CR><LF>");
    expect(toSymbolicText(bytes(0x0b, 0x4d, 0x53, 0x48, 0x1c, 0x0d))).toBe("<VT>MSH<FS><CR>");
    expect(toSymbolicText(bytes(0x05, 0x06, 0x15, 0x04))).toBe("<ENQ><ACK><NAK><EOT>");
  });

  it("shows bytes it cannot name as hex instead of guessing", () => {
    expect(toSymbolicText(bytes(0xe9, 0x14, 0x7f))).toBe("<0xE9><0x14><DEL>");
  });

  it("writes hex as upper-case pairs", () => {
    expect(toHex(bytes(0x02, 0xab, 0x00))).toBe("02 AB 00");
    expect(toHex(bytes())).toBe("");
  });

  it("decodes base64 and treats invalid input as no bytes", () => {
    expect(Array.from(decodeBase64("AjFIfA0="))).toEqual([0x02, 0x31, 0x48, 0x7c, 0x0d]);
    expect(decodeBase64("not base64!!").length).toBe(0);
  });

  describe("flowMarker", () => {
    const segment = (
      direction: TrafficSegment["direction"],
      flow?: TrafficSegment["flow"],
      role?: TrafficSegment["role"],
    ): TrafficSegment => ({
      seq: 1,
      at: "2026-11-06T12:00:00Z",
      direction,
      base64: "",
      flow,
      role,
    });

    it.each([
      ["received", "←", "Received"],
      ["response", "→", "Response"],
      ["sent", "→", "Sent"],
      ["reply", "←", "Reply"],
    ] as const)("marks the role %s as %s %s", (role, arrow, label) => {
      expect(flowMarker(segment("a_to_b", undefined, role))).toEqual({ arrow, label });
    });

    it.each([
      ["a_to_b", "inbound", "←", "Received"],
      ["a_to_b", "outbound", "→", "Response"],
      ["b_to_a", "outbound", "→", "Sent"],
      ["b_to_a", "inbound", "←", "Reply"],
    ] as const)("without a role, marks %s %s as %s %s", (direction, flow, arrow, label) => {
      expect(flowMarker(segment(direction, flow))).toEqual({ arrow, label });
    });

    it("reads a segment that does not say which way it went from its pane", () => {
      expect(flowMarker(segment("a_to_b"))).toEqual({ arrow: "←", label: "Received" });
      expect(flowMarker(segment("b_to_a"))).toEqual({ arrow: "→", label: "Sent" });
    });
  });
});
