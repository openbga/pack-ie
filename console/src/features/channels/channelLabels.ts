import type { PairingStrategy, RetentionPolicy, StopReason, TapFramer } from "@/api/types";
import { formatCount } from "@/lib/format";

export function stopReasonText(reason: StopReason): string {
  switch (reason) {
    case "paused":
      return "Paused by an operator";
    case "disabled":
      return "Disabled in the configuration";
    case "configuration":
      return "The configuration is invalid";
    case "fatal":
      return "Stopped after an error";
    case "replaced":
      return "Stopped because the configuration replaced or removed it";
    case "shutdown":
      return "Stopped because the engine is shutting down";
    case "store_unavailable":
      return "Waiting for the message store, which could not be opened; the engine keeps trying and starts the channel when it opens";
    case "not_implemented":
      return "This engine build does not run this kind of channel yet";
  }
}

const PAIRING: Record<PairingStrategy, string> = {
  parallel: "Parallel",
  wait_for_listener_then_pair: "Wait for the listener, then pair",
  receiver_then_sender: "Receiver, then sender",
};

export function pairingText(pairing: PairingStrategy): string {
  return PAIRING[pairing];
}

const FRAMERS: Record<TapFramer, string> = {
  none: "None (raw bytes)",
  astm: "ASTM",
  au: "AU",
  hl7_mllp: "HL7 MLLP",
};

export function framerText(framer: TapFramer): string {
  return FRAMERS[framer];
}

export function retentionText(retention: RetentionPolicy): string {
  const parts = [
    retention.days === null ? null : `${formatCount(retention.days)} days`,
    retention.maxMessages === null ? null : `${formatCount(retention.maxMessages)} messages`,
  ].filter((part) => part !== null);
  return parts.length === 0 ? "Not limited" : parts.join(" · ");
}
