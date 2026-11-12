/**
 * What the Messages list leaves in the navigation state when it opens a message, so the
 * record page's breadcrumb returns to the list with its filters, sort and page.
 */
export interface MessageListState {
  /** The list's search string, `?channel=…`, or empty. */
  messagesSearch: string;
}

/** The list's search string from a location's state; empty when the page was opened by a link. */
export function listSearchOf(state: unknown): string {
  if (typeof state !== "object" || state === null) return "";
  const search = (state as Partial<MessageListState>).messagesSearch;
  return typeof search === "string" && (search === "" || search.startsWith("?")) ? search : "";
}
