import { type RefObject, useLayoutEffect, useState } from "react";

/**
 * The width of an element in pixels, kept current as it resizes, or null while it cannot be
 * measured (before layout, or where there is no layout at all). The table fits its columns
 * to the space it has, which a window breakpoint cannot tell: the navigation and the help
 * drawer take some of it.
 */
export function useElementWidth(ref: RefObject<HTMLElement | null>): number | null {
  const [width, setWidth] = useState<number | null>(null);
  // Synchronizes with the element's layout, which lives outside React.
  useLayoutEffect(() => {
    const element = ref.current;
    if (element === null) return;
    const measure = (value: number) => setWidth(value > 0 ? Math.round(value) : null);
    measure(element.getBoundingClientRect().width);
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry !== undefined) measure(entry.contentRect.width);
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [ref]);
  return width;
}
