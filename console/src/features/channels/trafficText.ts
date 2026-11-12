import type { TrafficSegment } from "@/api/types";

/** Control characters as the names operators know from the protocols (ASTM, MLLP, serial). */
const CONTROL_NAMES: Readonly<Record<number, string>> = {
  0: "NUL",
  1: "SOH",
  2: "STX",
  3: "ETX",
  4: "EOT",
  5: "ENQ",
  6: "ACK",
  7: "BEL",
  8: "BS",
  9: "TAB",
  10: "LF",
  11: "VT",
  12: "FF",
  13: "CR",
  16: "DLE",
  17: "DC1",
  19: "DC3",
  21: "NAK",
  22: "SYN",
  23: "ETB",
  27: "ESC",
  28: "FS",
  29: "GS",
  30: "RS",
  31: "US",
  127: "DEL",
};

function hexByte(byte: number): string {
  return byte.toString(16).toUpperCase().padStart(2, "0");
}

/** The bytes of a base64 string; text that is not valid base64 yields no bytes. */
export function decodeBase64(base64: string): Uint8Array {
  try {
    const binary = atob(base64);
    const bytes = new Uint8Array(binary.length);
    for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
    return bytes;
  } catch {
    return new Uint8Array(0);
  }
}

/** `02 31 48 7C`: upper-case hex pairs, one space between. */
export function toHex(bytes: Uint8Array): string {
  return Array.from(bytes, hexByte).join(" ");
}

/**
 * The bytes as text, one byte per character: printable ASCII as is, control characters by
 * name (`<STX>`, `<CR><LF>`), and anything else (bytes of 0x80 and above, unnamed controls)
 * as `<0xNN>`, so what is on the wire is never guessed or hidden.
 */
export function toSymbolicText(bytes: Uint8Array): string {
  let text = "";
  for (const byte of bytes) {
    if (byte >= 0x20 && byte < 0x7f) text += String.fromCharCode(byte);
    else text += `<${CONTROL_NAMES[byte] ?? `0x${hexByte(byte)}`}>`;
  }
  return text;
}

export interface FlowMarker {
  /** `←` came in to the engine, `→` went out of it. Always shown beside its words. */
  arrow: "←" | "→";
  label: string;
}

const ROLE_MARKERS: Readonly<Record<NonNullable<TrafficSegment["role"]>, FlowMarker>> = {
  received: { arrow: "←", label: "Received" },
  response: { arrow: "→", label: "Response" },
  sent: { arrow: "→", label: "Sent" },
  reply: { arrow: "←", label: "Reply" },
};

/**
 * The marker of a message-channel segment, from its role: in the Source pane what the source
 * sent ("Received") and the engine's answer to it ("Response"); in the Destinations pane what
 * the engine sent ("Sent") and what the destination answered ("Reply"). A segment without a
 * role is read from the way it went and its pane.
 */
export function flowMarker(segment: TrafficSegment): FlowMarker {
  if (segment.role !== undefined) return ROLE_MARKERS[segment.role];
  const isSource = segment.direction === "a_to_b";
  const flow = segment.flow ?? (isSource ? "inbound" : "outbound");
  const role = isSource
    ? flow === "inbound"
      ? "received"
      : "response"
    : flow === "inbound"
      ? "reply"
      : "sent";
  return ROLE_MARKERS[role];
}
