import { useEffect, useRef } from "react";

/** How long typing must pause before a find or search runs. */
const TYPING_PAUSE_MS = 250;

/**
 * Calls `callback` once typing has paused (`later`), or at once (`now`), which cancels a
 * pending call. `cancel` drops a pending call too, for when the value changed from outside and
 * the typed text is stale. A pending call is also dropped when the component unmounts.
 */
export function useTypingPause(callback: (text: string) => void) {
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  // Synchronizes with the timer, outside React: a pending call must not outlive the field.
  useEffect(() => () => clearTimeout(timer.current), []);
  return {
    cancel: () => clearTimeout(timer.current),
    later: (text: string) => {
      clearTimeout(timer.current);
      timer.current = setTimeout(() => callback(text), TYPING_PAUSE_MS);
    },
    now: (text: string) => {
      clearTimeout(timer.current);
      callback(text);
    },
  };
}
