import {
  Activity,
  Bell,
  ClipboardList,
  GitBranch,
  LayoutDashboard,
  type LucideIcon,
  Mail,
  ScrollText,
  Settings,
  ShieldCheck,
  Unplug,
} from "lucide-react";

export interface Screen {
  path: string;
  /** Equal to the screen's page title, so the navigation and the `h1` say the same thing. */
  label: string;
  icon: LucideIcon;
  /** Set on Alerts: the navigation shows the number of open alerts beside it. */
  showsAlertCount?: boolean;
}

export interface NavSection {
  /** Absent for the dashboard, which stands alone above the work areas. */
  heading?: string;
  icon?: LucideIcon;
  screens: readonly Screen[];
}

export const HOME_PATH = "/dashboard";

/** docs/architecture.md §8: Dashboard, Channels, Endpoints, Messages, Logs, Alerts, Audit, Settings. */
export const navigation: readonly NavSection[] = [
  {
    screens: [{ path: "/dashboard", label: "Dashboard", icon: LayoutDashboard }],
  },
  {
    heading: "Operate",
    icon: Activity,
    screens: [
      { path: "/channels", label: "Channels", icon: GitBranch },
      { path: "/endpoints", label: "Endpoints", icon: Unplug },
      { path: "/messages", label: "Messages", icon: Mail },
    ],
  },
  {
    heading: "Diagnose",
    icon: ScrollText,
    screens: [
      { path: "/logs", label: "Logs", icon: ScrollText },
      { path: "/alerts", label: "Alerts", icon: Bell, showsAlertCount: true },
      { path: "/audit", label: "Audit", icon: ClipboardList },
      { path: "/transmission", label: "Transmission report", icon: ShieldCheck },
    ],
  },
  {
    heading: "Configure",
    icon: Settings,
    screens: [{ path: "/settings", label: "Settings", icon: Settings }],
  },
];
