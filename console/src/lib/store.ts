import type { StoreStatus } from "@/api/types";
import { formatBytes, formatCount } from "./format";

const NOT_REPORTED = "Not reported";

const STATE_LABELS: Record<StoreStatus["state"], string> = {
  ok: "Available",
  unavailable: "Unavailable",
  not_configured: "Not configured in this engine build",
};

/** The store's figures as words; a figure the engine does not report says so, never a zero. */
export function storeSummary(store: StoreStatus | null) {
  const bytes = (value: number | null | undefined) =>
    value === null || value === undefined ? NOT_REPORTED : formatBytes(value);
  const free = store?.diskFreeBytes ?? null;
  const total = store?.diskTotalBytes ?? null;
  let disk = bytes(free);
  if (free !== null && total !== null && total > 0) {
    disk = `${formatBytes(free)} of ${formatBytes(total)} (${Math.round((free / total) * 100)}%)`;
  }
  return {
    state: store === null ? NOT_REPORTED : STATE_LABELS[store.state],
    messages:
      store?.messageCount === null || store === null
        ? NOT_REPORTED
        : formatCount(store.messageCount),
    size: bytes(store?.databaseBytes),
    disk,
  };
}
