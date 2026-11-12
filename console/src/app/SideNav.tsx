import { useState } from "react";
import { useLocation } from "react-router";
import { useSnapshot } from "@/api/hooks";
import { NavGroup } from "./NavGroup";
import { NavItem, type QueueCount } from "./NavItem";
import { type NavSection, navigation, type Screen } from "./navigation";
import styles from "./SideNav.module.css";

type OpenSections = Record<string, boolean>;

/**
 * The side navigation: the work areas, each collapsible and open at first, with the number of
 * open alerts beside Alerts. It scrolls on its own, so the top bar stays in place.
 */
export function SideNav({ collapsed }: { collapsed: boolean }) {
  const { pathname } = useLocation();
  const current = currentScreen(pathname);
  const active = navigation.find(
    (section) => current !== undefined && section.screens.includes(current),
  );
  const [open, toggle] = useOpenSections();
  const alertCount = useAlertCount();
  const items = (screens: readonly Screen[], onNavigate?: () => void, compact = false) =>
    screens.map((screen) => (
      <NavItem
        key={screen.path}
        screen={screen}
        current={screen === current ? currentKind(screen, pathname) : undefined}
        count={screen.showsAlertCount ? alertCount : null}
        compact={compact}
        onNavigate={onNavigate}
      />
    ));
  return (
    <nav
      id="main-navigation"
      aria-label="Main"
      className={styles.nav}
      data-compact={collapsed || undefined}
    >
      <ul className={styles.list}>
        {navigation.map((section) =>
          section.heading === undefined ? (
            items(section.screens, undefined, collapsed)
          ) : (
            <NavGroup
              key={section.heading}
              heading={section.heading}
              icon={section.icon}
              compact={collapsed}
              active={section === active}
              count={section.screens.some((screen) => screen.showsAlertCount) ? alertCount : null}
              isOpen={open(section)}
              onToggle={() => toggle(section)}
            >
              {(onNavigate) => items(section.screens, onNavigate)}
            </NavGroup>
          ),
        )}
      </ul>
    </nav>
  );
}

/** The one entry marked current: the screen itself, or the list a detail screen sits under. */
function currentScreen(pathname: string): Screen | undefined {
  return navigation
    .flatMap((section) => section.screens)
    .filter((screen) => pathname === screen.path || pathname.startsWith(`${screen.path}/`))
    .sort((a, b) => b.path.length - a.path.length)[0];
}

function currentKind(screen: Screen, pathname: string): "page" | "true" {
  return pathname === screen.path ? "page" : "true";
}

/** Work areas start open; the operator's choice lasts until the console closes. */
function useOpenSections(): [(section: NavSection) => boolean, (section: NavSection) => void] {
  const [closed, setClosed] = useState<OpenSections>({});
  const isOpen = (section: NavSection) => !closed[section.heading ?? ""];
  const toggle = (section: NavSection) =>
    setClosed((last) => ({ ...last, [section.heading ?? ""]: isOpen(section) }));
  return [isOpen, toggle];
}

function useAlertCount(): QueueCount | null {
  const snapshot = useSnapshot();
  const open = snapshot.data?.openAlerts.length ?? 0;
  if (open === 0) return null;
  return { shown: String(open), spoken: `${open} open` };
}
