import type { LogLevel } from "@/api/types";

/** How a level reads on a row and in a filter. */
export const LEVEL_LABELS: Readonly<Record<LogLevel, string>> = {
  error: "Error",
  warn: "Warning",
  info: "Info",
  debug: "Debug",
  trace: "Trace",
};

/** The minimum-level choices: each includes everything more severe. */
export const LEVEL_CHOICES: readonly { id: LogLevel; label: string }[] = [
  { id: "error", label: "Error only" },
  { id: "warn", label: "Warning and above" },
  { id: "info", label: "Info and above" },
  { id: "debug", label: "Debug and above" },
  { id: "trace", label: "Everything (trace)" },
];
