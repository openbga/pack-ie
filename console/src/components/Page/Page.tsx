import type { ReactNode } from "react";
import styles from "./Page.module.css";

/**
 * The frame of one screen: a `PageHeader` followed by the content, in a column at least as
 * tall as the window's main area. A list page's main `DataGrid` with `fill` takes the
 * remaining height, so the page does not scroll around a scrolling grid.
 */
export function Page({
  children,
  layout,
}: {
  children: ReactNode;
  /** A title, compact collection toolbar and one growing grid. */
  layout?: "collection";
}) {
  return (
    <div className={styles.page} data-layout={layout}>
      {children}
    </div>
  );
}
