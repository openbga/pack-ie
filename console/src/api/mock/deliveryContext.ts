import { K_ATTENTION, K_CLOSED, K_OK, K_PENDING, K_RESENT } from "./messageKinds";
import type { MessageProfile } from "./messageProfiles";

/** How the deliveries of a kind of message stand; null for kinds that have none. */
export function planOf(kind: number): DeliveryContext["plan"] | null {
  switch (kind) {
    case K_OK:
    case K_RESENT:
      return "ok";
    case K_ATTENTION:
      return "attention";
    case K_CLOSED:
      return "closed";
    case K_PENDING:
      return "pending";
    default:
      return null;
  }
}

/** What the generator needs to know to give one message its deliveries. */
export interface DeliveryContext {
  profile: MessageProfile;
  /** How the deliveries of the message stand. */
  plan: "ok" | "attention" | "closed" | "pending";
  seed: number;
  /** The message's index; keeps ids and hashes apart between messages. */
  index: number;
  idPrefix: string;
  receivedAtMs: number;
  control: string;
  configVersion: number;
}
