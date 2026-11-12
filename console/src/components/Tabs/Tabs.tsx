import type { ReactNode } from "react";
import { Tabs as AriaTabs, type LinkProps, Tab, TabList, TabPanel } from "react-aria-components";
import styles from "./Tabs.module.css";

/** One tab: its own route, so Back, reload and links work. */
interface TabLink {
  id: string;
  label: string;
  href: string;
}

interface TabsProps {
  /** Names the tab list, such as "Backups" or "Method SYN-M1". */
  label: string;
  /** At most 6, the main task first. */
  tabs: readonly TabLink[];
  /** The tab whose route is shown, taken from the URL. */
  selectedId: string;
  /** Options for every tab's navigation, such as `useRecordSequence().keep`. */
  routerOptions?: LinkProps["routerOptions"];
  /** The selected tab's content, usually the route's `<Outlet />`. */
  children: ReactNode;
  /** A tab whose body is collection controls and a growing grid. */
  contentLayout?: "collection";
}

/**
 * Peer views of one record or subject, one route per tab. The selected tab follows the URL.
 * Arrow keys only move focus between tabs; a click, Enter or Space opens one, because a tab
 * may load records. Render it in the parent route with the tab routes below it, so it stays
 * mounted and keeps focus when its route changes.
 */
export function Tabs({
  label,
  tabs,
  selectedId,
  routerOptions,
  children,
  contentLayout,
}: TabsProps) {
  return (
    <AriaTabs selectedKey={selectedId} keyboardActivation="manual" className={styles.tabs}>
      <TabList aria-label={label} className={styles.tabList}>
        {tabs.map((tab) => (
          <Tab
            key={tab.id}
            id={tab.id}
            href={tab.href}
            routerOptions={routerOptions}
            className={styles.tab}
          >
            {tab.label}
          </Tab>
        ))}
      </TabList>
      {/* Keyed, so the panel's id follows the selected tab and its `aria-controls`. */}
      <TabPanel
        key={selectedId}
        id={selectedId}
        className={styles.panel}
        data-layout={contentLayout}
      >
        {children}
      </TabPanel>
    </AriaTabs>
  );
}
