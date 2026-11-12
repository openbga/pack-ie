// Feeding a message channel over MLLP for `liveMessages.test.tsx`: synthetic HL7, no patient data.

import { loadNet } from "./liveHarness";

// ── Feeding the channel ──────────────────────────────────────────────────────────────────

const SB = "\x0b";
const EB = "\x1c";

/** One MLLP exchange: the framed message out, the framed answer back, as its segments. */
export async function sendMllp(port: number, segments: string[]): Promise<string[]> {
  const net = await loadNet();
  const socket = net.createConnection({ host: "127.0.0.1", port });
  try {
    await new Promise((resolve, reject) => {
      socket.on("connect", resolve);
      socket.on("error", reject);
    });
    const answer = new Promise<string>((resolve, reject) => {
      let text = "";
      const timer = setTimeout(() => reject(new Error(`no MLLP answer; got ${text}`)), 10_000);
      socket.on("data", (chunk) => {
        text += new TextDecoder().decode(chunk);
        if (text.includes(`${EB}\r`)) {
          clearTimeout(timer);
          resolve(text);
        }
      });
      socket.on("error", reject);
    });
    socket.write(`${SB}${segments.join("\r")}\r${EB}\r`);
    const framed = await answer;
    return framed.replace(SB, "").replace(`${EB}\r`, "").split("\r");
  } finally {
    socket.destroy();
  }
}

/** The acknowledgement code of an HL7 answer (`MSA-1`). */
export const ackCode = (segments: string[]) =>
  segments.find((segment) => segment.startsWith("MSA|"))?.split("|")[1] ?? "";

/** A synthetic result message. The identifiers are made up; there is no patient data. */
export function result(control: string, sample: string): string[] {
  return [
    `MSH|^~\\&|SIMLAB|SIMFAC|PACKIE|PACKIEFAC|20260101120000||ORU^R01|${control}|P|2.5.1`,
    `PID|1||PID-TEST-LIVE||TEST^PATIENT||19700101|F`,
    `OBR|1||${sample}|GLU^Glucose`,
    "OBX|1|NM|GLU^Glucose||95|mg/dL|70-110|N|||F|||20260101115900",
  ];
}
