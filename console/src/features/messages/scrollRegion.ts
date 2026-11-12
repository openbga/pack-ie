/**
 * A scrolling body is a named region in the Tab order, so the keyboard can scroll it (WCAG
 * 2.1.1). Spread, because the lint rules for focusable landmarks do not know this case.
 */
export const scrollRegion = (label: string) => ({
  role: "region",
  "aria-label": label,
  tabIndex: 0,
});
