import type { DeliveryChip, MessageSummary } from "@/api/types";
import { StatusBadge } from "@/components/StatusBadge";
import styles from "./DeliveryChips.module.css";
import { ackLevelText, chipBadgeText, chipText, deliveryChipStatus } from "./messageWording";

/** Chips shown in a row; the rest are counted and named in the "+N more" title. */
const MAX_CHIPS = 2;

const chipTitle = (chip: DeliveryChip) => `${chipText(chip)}. ${ackLevelText(chip.ackLevel)}`;

/**
 * The delivery summary of a message: one chip per destination, each on its own line, with
 * the delivery state's icon and label (never colour alone) and the destination's name, which
 * ends in "…" when it does not fit. An acknowledged delivery says what the acknowledgement
 * proved; the full wording is the chip's tooltip. At most `MAX_CHIPS` chips show, so a row has
 * a bounded height; the others are counted ("+1 more") and listed in that count's tooltip.
 */
export function DeliveryChips({
  message,
}: {
  message: Pick<MessageSummary, "deliveries" | "state">;
}) {
  const { deliveries } = message;
  if (deliveries.length === 0) {
    return <span className={styles.none}>No deliveries</span>;
  }
  const hidden = deliveries.slice(MAX_CHIPS);
  return (
    <ul className={styles.chips} aria-label="Deliveries">
      {deliveries.slice(0, MAX_CHIPS).map((chip) => (
        <li key={chip.deliveryId} className={styles.chip} title={chipTitle(chip)}>
          <StatusBadge tone={deliveryChipStatus(chip.state).tone}>
            {chipBadgeText(chip)}
          </StatusBadge>
          <span className={styles.destination}>{chip.destinationLabel}</span>
        </li>
      ))}
      {hidden.length > 0 && (
        <li className={styles.more} title={hidden.map(chipText).join("\n")}>
          +{hidden.length} more
        </li>
      )}
    </ul>
  );
}
