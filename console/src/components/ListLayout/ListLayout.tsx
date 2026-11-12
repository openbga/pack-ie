import type { ReactNode, Ref } from "react";
import styles from "./ListLayout.module.css";

interface ListLayoutProps {
  /** The list's element, for measuring its width. */
  listRef?: Ref<HTMLDivElement>;
  /** The open record, beside the list. */
  panel: ReactNode;
  /** The grid, and a `ListFooter` below it. */
  children: ReactNode;
}

/** A grid with its record panel beside it, filling the page's height. */
export function ListLayout({ listRef, panel, children }: ListLayoutProps) {
  return (
    <div className={styles.layout}>
      <div ref={listRef} className={styles.list}>
        {children}
      </div>
      {panel}
    </div>
  );
}

/** The line below the grid: how to reach older records. */
export function ListFooter({ children }: { children: ReactNode }) {
  return <div className={styles.footer}>{children}</div>;
}

/** A note below "no results", such as what a filter matches. */
export function ListHint({ children }: { children: ReactNode }) {
  return <p className={styles.hint}>{children}</p>;
}
