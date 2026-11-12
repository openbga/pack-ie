/**
 * After Continue, focus returns to the element that had it before the cover (React Aria does
 * that) when that element is still on the page. When it is gone, because the screen was left,
 * focus would fall to the document: put it on the page's heading, else on the main region, as
 * a navigation does.
 */
export function restoreFocusAfterIdle(): void {
  // React Aria restores focus on the next frame; look after it has had its turn.
  requestAnimationFrame(() =>
    requestAnimationFrame(() => {
      const active = document.activeElement;
      if (active !== null && active !== document.body) return;
      const target =
        document.querySelector<HTMLElement>("main h1") ??
        document.querySelector<HTMLElement>("main");
      if (target === null) return;
      target.tabIndex = -1;
      target.focus();
    }),
  );
}
