import { useLayoutEffect, useRef } from "react";

/**
 * The value as of the latest render, for a callback that runs later, such as a find that
 * reports after a pause in typing: it must change what is current by then.
 */
export function useLatest<T>(value: T) {
  const latest = useRef(value);
  useLayoutEffect(() => {
    latest.current = value;
  });
  return latest;
}
