import {
  isMessageIdPrefix,
  isSearchableText,
  type MessageFilters,
  QUICK_FILTERS,
  RELATIVE_MS,
} from "./messageParams";
import { deliveryLabel, directionText, messageStateStatus } from "./messageWording";

const RANGE_WORDS: Readonly<Record<string, string>> = {
  "-1h": "the last hour",
  "-24h": "the last 24 hours",
  "-7d": "the last 7 days",
  "-30d": "the last 30 days",
};

/** The time range in words: a quick range by its name, anything else as a custom range. */
function rangeWords({ from, to }: MessageFilters): string {
  if (from === undefined && to === undefined) return "received at any time";
  if (to === undefined && from !== undefined && from in RELATIVE_MS) {
    return `received in ${RANGE_WORDS[from]}`;
  }
  return "received in a custom time range";
}

/**
 * The filters of the Messages list that are in effect, in words and in the order the filter bar
 * shows them: the time range always, then each criterion that is set. A text or message ID too
 * short to search is not in effect, so it is not listed.
 */
export function filterWords(
  filters: MessageFilters,
  channelLabel: (id: string) => string,
): string[] {
  const words = [rangeWords(filters)];
  const id = filters.id?.trim();
  if (id) words.push(`identifier ${filters.idExact ? "is" : "starts with"} ${id}`);
  if (isSearchableText(filters.text)) words.push(`text contains ${filters.text.trim()}`);
  if (filters.channels.length > 0) {
    words.push(`channel ${filters.channels.map(channelLabel).join(" or ")}`);
  }
  if (filters.direction) words.push(`direction ${directionText(filters.direction)}`);
  if (filters.quick) {
    words.push(`quick filter ${QUICK_FILTERS[filters.quick].label}`);
  } else {
    if (filters.state) words.push(`message state ${messageStateStatus(filters.state).label}`);
    if (filters.delivery) words.push(`delivery state ${deliveryLabel(filters.delivery)}`);
  }
  if (isMessageIdPrefix(filters.messageId)) {
    words.push(`message ID starts with ${filters.messageId.trim()}`);
  }
  return words;
}
