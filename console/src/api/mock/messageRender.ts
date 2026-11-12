import type { BodyFormat } from "../types";

/** The engine's display limit for one rendered body. */
export const DISPLAY_LIMIT_BYTES = 64 * 1024;

const CONTROL_NAMES = [
  "NUL",
  "SOH",
  "STX",
  "ETX",
  "EOT",
  "ENQ",
  "ACK",
  "BEL",
  "BS",
  "HT",
  "LF",
  "VT",
  "FF",
  "CR",
  "SO",
  "SI",
  "DLE",
  "DC1",
  "DC2",
  "DC3",
  "DC4",
  "NAK",
  "SYN",
  "ETB",
  "CAN",
  "EM",
  "SUB",
  "ESC",
  "FS",
  "GS",
  "RS",
  "US",
];

/** `packie_core::visible_text`: control characters by name, a line break after CR and LF. */
export function visibleText(text: string): string {
  let out = "";
  for (const char of text) {
    const code = char.codePointAt(0) ?? 0;
    if (code < CONTROL_NAMES.length) {
      out += `<${CONTROL_NAMES[code]}>`;
      if (code === 0x0d || code === 0x0a) out += "\n";
    } else {
      out += code === 0x7f ? "<DEL>" : char;
    }
  }
  return out;
}

/** `packie_core::hex_dump`: offset, 16 bytes in hex, and the printable ASCII, a line per 16 bytes. */
export function hexDump(text: string): string {
  let out = "";
  for (let start = 0; start < text.length; start += 16) {
    const chunk = Array.from(text.slice(start, start + 16), (char) => char.charCodeAt(0) & 0xff);
    const hex = chunk.map((byte) => byte.toString(16).padStart(2, "0")).join(" ");
    const ascii = chunk
      .map((byte) => (byte >= 0x20 && byte < 0x7f ? String.fromCharCode(byte) : "."))
      .join("");
    out += `${start.toString(16).padStart(8, "0")}  ${hex.padEnd(47)}  ${ascii}\n`;
  }
  return out;
}

/** A body as the engine renders it for `get_message_body`, cut at the display limit. */
export function renderBody(
  text: string,
  format: BodyFormat,
): { content: string; truncated: boolean } {
  const truncated = text.length > DISPLAY_LIMIT_BYTES;
  const shown = truncated ? text.slice(0, DISPLAY_LIMIT_BYTES) : text;
  return { content: format === "hex" ? hexDump(shown) : visibleText(shown), truncated };
}
