import type { Locator, Page } from "@playwright/test";
import { BRIDGE_CHANNEL, ENDPOINT, MESSAGE_CHANNEL } from "./states.support";

type Present = (page: Page) => Locator;

export interface Spot {
  name: string;
  route: string;
  topic: string;
  /** Words the article uses for things on the page. */
  mentions: (string | RegExp)[];
  /** The same things, on the page. */
  present: Present[];
  /** What the article says is absent or never offered, and the page must not offer. */
  absent?: Present[];
}

const inMain = (page: Page): Locator => page.getByRole("main");
const byRole =
  (role: "button" | "link" | "searchbox" | "heading") =>
  (name: string | RegExp): Present =>
  (page) =>
    inMain(page).getByRole(role, { name }).first();
const button = byRole("button");
const link = byRole("link");
const searchbox = byRole("searchbox");
const headingOf = byRole("heading");
const shows =
  (value: string | RegExp): Present =>
  (page) =>
    inMain(page).getByText(value).first();
const columnHeader =
  (name: string): Present =>
  (page) =>
    inMain(page).getByRole("columnheader", { name, exact: true }).first();

export const SPOTS: readonly Spot[] = [
  {
    name: "Dashboard",
    route: "/dashboard",
    topic: "Dashboard",
    mentions: ["Endpoints", "Traffic", "Last activity", "Queue", "Retry", "Critical alert"],
    present: [
      columnHeader("Channel"),
      columnHeader("State"),
      columnHeader("Endpoints"),
      columnHeader("Traffic"),
      columnHeader("Queue"),
      shows("Critical alert"),
      shows("Unresolved deliveries"),
    ],
  },
  {
    name: "Channels",
    route: "/channels",
    topic: "Channels",
    mentions: ["Needs attention only", "Add channel", "Pause", "Restart", "Edit"],
    present: [
      (page) => inMain(page).getByRole("checkbox", { name: "Needs attention only" }),
      link("Add channel"),
      columnHeader("Kind"),
      columnHeader("State"),
      columnHeader("Endpoints"),
      button(/^Actions for /),
    ],
  },
  {
    name: "Endpoints",
    route: "/endpoints",
    topic: "Endpoints",
    mentions: ["Add endpoint", "Kind", "State", "Last activity"],
    present: [
      link("Add endpoint"),
      searchbox("Find endpoints"),
      columnHeader("Kind"),
      columnHeader("State"),
      columnHeader("Last activity"),
    ],
  },
  {
    name: "Messages",
    route: "/messages",
    topic: "Messages",
    mentions: [
      "Identifier",
      "Message text",
      "More filters",
      "Needs attention",
      "Queued",
      "Duplicates",
      "Invalid or aborted",
      "Columns",
      "Export…",
      "Message state",
      "Delivery summary",
    ],
    present: [
      searchbox("Identifier"),
      searchbox("Message text"),
      button("More filters"),
      button(/^Needs attention/),
      button(/^Queued/),
      button(/^Duplicates/),
      button(/^Invalid or aborted/),
      button(/^Columns/),
      button("Export…"),
      columnHeader("Message state"),
      columnHeader("Delivery summary"),
    ],
  },
  {
    name: "Logs",
    route: "/logs",
    topic: "Logs",
    mentions: ["Follow", "Jump to live", "Load older", "Level", "Event", "Channel", "Endpoint"],
    present: [
      button("Follow"),
      button("Load older events"),
      searchbox("Search log messages"),
      (page) => inMain(page).getByRole("combobox", { name: "Event" }),
      shows("Level"),
      shows("Channel"),
      shows("Endpoint"),
      shows("Time range"),
    ],
  },
  {
    name: "Alerts",
    route: "/alerts",
    topic: "Alerts",
    mentions: ["severity", "State", "Next step", "Concerns", "First seen", "Last seen"],
    present: [
      searchbox("Find alerts"),
      shows("State"),
      shows("Severity"),
      columnHeader("Next step"),
      columnHeader("Concerns"),
      columnHeader("First seen"),
      columnHeader("Last seen"),
    ],
    // "An alert cannot be dismissed; it clears by itself."
    absent: [button(/dismiss|acknowledge|snooze/i)],
  },
  {
    name: "Audit",
    route: "/audit",
    topic: "Audit",
    mentions: [
      "Time range",
      "Actor",
      "Object ID",
      "Outcome",
      "Load older events",
      "Export…",
      "Verify chain",
      "audit.anchor",
    ],
    present: [
      searchbox("Actor"),
      searchbox("Object ID"),
      shows("Time range"),
      shows("Object type"),
      shows("Outcome"),
      button("Load older events"),
      button("Export…"),
      button("Verify chain"),
      button(/^Columns:/),
      columnHeader("Outcome"),
    ],
  },
  {
    name: "Transmission report",
    route: `/transmission?channel=${MESSAGE_CHANNEL.id}`,
    topic: "Transmission report",
    mentions: ["Run report", "Period", "Destination", "Written, not confirmed", "Export CSV…"],
    present: [button("Run report"), shows("Period"), shows("Channel")],
  },
  {
    name: "Message channel",
    route: `/channels/${MESSAGE_CHANNEL.id}`,
    topic: "Channel",
    mentions: ["Pause", "Restart", "Resume", "Live transcript", "Recent messages", "destination"],
    present: [
      button("Pause"),
      button("Restart"),
      link("Edit"),
      headingOf("Recent messages"),
      headingOf("Live transcript"),
      button("Start live transcript"),
      headingOf("Destinations and deliveries"),
    ],
    // "A button is missing when the engine does not allow it now": a running channel has no Resume.
    absent: [button("Resume")],
  },
  {
    name: "Bridge channel",
    route: `/channels/${BRIDGE_CHANNEL.id}`,
    topic: "Channel",
    mentions: ["side A", "side B", "buffer", "Pause", "Restart"],
    present: [
      headingOf("Buffers"),
      shows("Side A"),
      shows("Side B"),
      button("Pause"),
      button("Restart"),
    ],
  },
  {
    name: "Endpoint",
    route: `/endpoints/${ENDPOINT.id}`,
    topic: "Endpoint",
    mentions: [
      "Restart the channels that use it",
      "Edit",
      "failure streak",
      "next retry",
      "Test connection",
    ],
    present: [
      button("Restart the channels that use it"),
      button("Test connection"),
      link("Edit"),
      shows("Failure streak"),
      shows("Next retry"),
      shows("Held by"),
      headingOf("Channels using this endpoint"),
    ],
  },
  {
    name: "Message",
    route: "/messages/msg-7",
    topic: "Message",
    mentions: ["Show original message", "Resend", "newer revision", "Close"],
    present: [
      button("Show original message"),
      button("Resend…"),
      button(/^Close delivery to /),
      headingOf("Deliveries"),
    ],
  },
  {
    name: "Settings Engine",
    route: "/settings/engine",
    topic: "Settings",
    mentions: ["bootstrap.json", "Alert thresholds", "node name"],
    present: [shows(/bootstrap\.json/), shows("Node name"), button("Save thresholds")],
  },
  {
    name: "Settings Retention",
    route: "/settings/retention",
    topic: "Settings",
    mentions: ["message store", "free disk space"],
    present: [shows("Message store"), shows("Database size"), shows("Disk free")],
  },
  {
    name: "Settings Secrets",
    route: "/settings/secrets",
    topic: "Settings",
    mentions: ["add one", "never shown again"],
    present: [button("Add secret"), shows(/never shown/)],
  },
  {
    name: "Settings About",
    route: "/settings/about",
    topic: "Settings",
    mentions: ["engine, console and protocol versions", "build", "platform", "licence"],
    present: [
      shows("Engine version"),
      shows("Console version"),
      shows("Protocol version"),
      shows("Build commit"),
      shows("Platform"),
      shows("Licence"),
    ],
  },
  {
    name: "Settings Backup",
    route: "/settings/backup",
    topic: "Backup",
    mentions: ["Back up now", "Up to date", "Stale", "No backup yet", "packie backup restore"],
    present: [
      button("Back up now"),
      headingOf("Backup status"),
      headingOf("Restore"),
      shows("Up to date"),
      shows(/packie --bootstrap .* backup restore/),
    ],
  },
  {
    name: "Settings Notifications",
    route: "/settings/notifications",
    topic: "Notifications",
    mentions: ["No target is failing", "bootstrap.json", "Send test", "audit log"],
    present: [
      shows("No target is failing"),
      headingOf("Targets"),
      button("Send test…"),
      shows(/bootstrap\.json/),
    ],
  },
  {
    name: "Settings Versions",
    route: "/settings/versions?version=46",
    topic: "Saving and versions",
    mentions: ["Roll back", "Settings › Versions", "comparison"],
    present: [button("Roll back to this version…"), headingOf(/^Compared with the active version/)],
  },
  {
    name: "New channel",
    route: "/channels/new",
    topic: "Channel editor",
    mentions: ["Bridge", "Message channel"],
    present: [button(/^Select… Kind/), button("Validate"), button("Save and apply")],
  },
  {
    name: "Edit bridge channel",
    route: `/channels/${BRIDGE_CHANNEL.id}/edit`,
    topic: "Channel editor",
    mentions: [
      "Pairing strategy",
      "Buffer size",
      "Tap framer",
      "Reconnect policy",
      "maximum attempts",
      "Endpoint A",
      "Endpoint B",
    ],
    present: [
      shows("Pairing strategy"),
      shows("Buffer size (bytes)"),
      shows("Tap framer"),
      shows("Reconnect policy"),
      shows("Maximum attempts"),
      shows("Endpoint A"),
      shows("Endpoint B"),
      button("Validate"),
    ],
  },
  {
    name: "Edit message channel",
    route: `/channels/${MESSAGE_CHANNEL.id}/edit`,
    topic: "Channel editor",
    mentions: [
      "Detect duplicate messages",
      "Source filters",
      "Keep for (days)",
      "Keep at most (messages)",
      "destination",
    ],
    present: [
      shows("Detect duplicate messages"),
      shows("Source filters"),
      shows("Keep for (days)"),
      shows("Keep at most (messages)"),
      shows("Destinations"),
      button("Delete channel…"),
    ],
  },
  {
    name: "Edit TCP endpoint",
    route: `/endpoints/${ENDPOINT.id}/edit`,
    topic: "Endpoint editor",
    mentions: [
      "Idle timeout",
      "Allow any peer to connect",
      "Keepalive",
      "allowlist|addresses",
      "Test connection",
    ],
    present: [
      button("Test connection"),
      shows("Idle timeout (seconds)"),
      shows("Send TCP keepalive probes"),
      shows("Allow any peer to connect"),
      shows("Peer allowlist"),
      button("Delete endpoint…"),
    ],
  },
  {
    name: "Edit SFTP endpoint",
    route: "/endpoints/ep-reflab-sftp/edit",
    topic: "Endpoint editor",
    mentions: [
      "user name",
      "remote path",
      "poll interval",
      "host key fingerprint",
      "File name pattern",
    ],
    present: [
      shows("User name"),
      shows("Remote path"),
      shows("Poll interval (seconds)"),
      shows("Host key fingerprint"),
      shows("File name pattern"),
      shows("Password secret name"),
    ],
  },
  {
    name: "Edit serial endpoint",
    route: "/endpoints/ep-a-au5800/edit",
    topic: "Endpoint editor",
    mentions: ["Refresh", "used by", "type any name"],
    present: [(page) => inMain(page).getByRole("combobox", { name: /^Port/ }), button("Refresh")],
  },
  {
    name: "New endpoint",
    route: "/endpoints/new",
    topic: "Endpoint editor",
    mentions: ["Serial", "TCP client", "TCP listener"],
    present: [button(/^Select… Kind/), button("Validate"), button("Save and apply")],
  },
];
