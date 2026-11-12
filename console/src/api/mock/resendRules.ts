import type { Delivery, Rejection, ResendRequest } from "../types";
import { isFinal, MAY_BE_PROCESSED } from "./deliveries";
import type { DestinationInfo } from "./messageProfiles";

const NEW_MESSAGE_ONLY = new Set(["invalid", "aborted", "filtered", "duplicate"]);

const reject = (code: Rejection["code"], message: string): Rejection => ({ code, message });

/** Whether `delivery` can be resent in `mode`, as the engine judges it (spec §9). */
export function judgeResend(
  request: ResendRequest,
  state: string,
  existing: Delivery | undefined,
  destination: DestinationInfo | undefined,
): Rejection | null {
  if (destination === undefined || !destination.enabled) {
    return reject(
      "destination_not_found",
      "Destination is absent or disabled in the active configuration.",
    );
  }
  if (request.mode === "same_message") {
    if (NEW_MESSAGE_ONLY.has(state)) {
      return reject("invalid_state", "This message can only be resent as a new message.");
    }
    if (existing === undefined) {
      return reject("invalid_state", "No existing delivery; resend as a new message.");
    }
    if (!isFinal(existing.state)) {
      return reject("delivery_in_progress", "Wait for the existing delivery to finish.");
    }
  }
  if (
    existing !== undefined &&
    MAY_BE_PROCESSED.has(existing.state) &&
    !request.confirmDuplicateRisk
  ) {
    return reject(
      "duplicate_risk_unconfirmed",
      "Check the destination and confirm the risk of duplicate delivery.",
    );
  }
  return null;
}
