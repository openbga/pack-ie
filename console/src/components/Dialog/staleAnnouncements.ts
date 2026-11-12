import { useEffect } from "react";

/** React Aria's live regions: it keeps one `data-live-announcer` element for the whole page. */
const ANNOUNCER = "[data-live-announcer]";

/** How long after a dialog closes an announcement may still arrive (the announcer is created 100 ms late). */
const WATCH_MS = 500;

/** Removes announcements that point (aria-labelledby) at elements no longer in the page. */
function dropStaleAnnouncements(): void {
  for (const note of document.querySelectorAll(`${ANNOUNCER} [aria-labelledby]`)) {
    const ids = (note.getAttribute("aria-labelledby") ?? "").split(/\s+/).filter(Boolean);
    if (ids.some((id) => document.getElementById(id) === null)) note.remove();
  }
}

/**
 * A pending button announces its state through a live region whose entry names the button by
 * id, and keeps the entry for seven seconds. A dialog that closes with its command takes the
 * button with it, and the entry would name nothing: axe reports `role-img-alt`, and a screen
 * reader has nothing to read. Mount this inside a dialog: when the dialog goes, entries for
 * what went with it go too, including one that arrives a moment later.
 */
export function useDropStaleAnnouncements(): void {
  useEffect(
    () => () => {
      // Synchronizes with the live regions React Aria keeps outside React.
      dropStaleAnnouncements();
      const watch = new MutationObserver(dropStaleAnnouncements);
      watch.observe(document.body, { childList: true, subtree: true });
      setTimeout(() => watch.disconnect(), WATCH_MS);
    },
    [],
  );
}
