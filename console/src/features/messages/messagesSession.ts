import { DEFAULT_PAGE_SIZE, type SensitiveFilters } from "./messageParams";

/**
 * What the Messages list keeps while the console is open and the operator moves around it, so
 * a message opened from the list and the way back leave the working set as it was. It lives in
 * this module's memory only: never in the URL, local storage or the engine, because the
 * identifier and text criteria can hold patient data. Closing or reloading the console
 * forgets it.
 */
interface MessagesSession {
  /** The identifier, text and message ID criteria. */
  criteria: SensitiveFilters;
  /** Rows per page; the page number is in the URL, and means nothing without it. */
  pageSize: number;
  /** The columns chosen in the column chooser; `null` is the default set that fits. */
  columns: readonly string[] | null;
}

const fresh = (): MessagesSession => ({
  criteria: {},
  pageSize: DEFAULT_PAGE_SIZE,
  columns: null,
});

let session = fresh();

export const messagesSession = (): MessagesSession => session;

export function keepMessagesSession(change: Partial<MessagesSession>): void {
  session = { ...session, ...change };
}

/** Forgets everything, as closing the console does; for tests. */
export function forgetMessagesSession(): void {
  session = fresh();
}
