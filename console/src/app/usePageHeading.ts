import { type RefObject, useLayoutEffect, useRef } from "react";
import { useLocation } from "react-router";

export const APP_NAME = "PACK-IE Console";

/**
 * Keeps the window title equal to the page's `h1` and, after each navigation, moves focus to
 * that heading (WCAG 2.2 SC 2.4.2, 2.4.3), unless the new screen already put focus in itself
 * (a scan field, for example). The first screen shown leaves focus alone, also behind a
 * redirect. The heading can arrive after the route, once the screen's data has loaded, so the
 * region is observed.
 */
export function usePageHeading(region: RefObject<HTMLElement | null>) {
  const { pathname } = useLocation();
  // The path whose heading was last shown; null until the first screen appears.
  const shownPath = useRef<string | null>(null);
  // A layout effect, so the previous screen's observer stops in the same commit, before it can
  // see the next screen's heading.
  useLayoutEffect(() => {
    const element = region.current;
    if (!element) return;
    let focusPending = shownPath.current !== null && shownPath.current !== pathname;
    const sync = () => {
      const heading = element.querySelector("h1");
      setTitle(heading);
      if (!heading) return;
      shownPath.current = pathname;
      if (focusPending) {
        focusPending = false;
        focusUnlessInside(element, heading);
      }
    };
    sync();
    const observer = new MutationObserver(sync);
    observer.observe(element, { childList: true, subtree: true, characterData: true });
    return () => {
      observer.disconnect();
      document.title = APP_NAME;
    };
  }, [region, pathname]);
}

function setTitle(heading: HTMLHeadingElement | null) {
  const text = heading?.textContent?.trim();
  const title = text ? `${text} · ${APP_NAME}` : APP_NAME;
  if (document.title !== title) document.title = title;
}

function focusUnlessInside(region: HTMLElement, heading: HTMLHeadingElement) {
  if (region.contains(document.activeElement)) return;
  heading.tabIndex = -1;
  heading.focus();
}
