// Client-side checks are limited to presence and format; the engine validates everything again.
import type { TextFormat } from "@/components/FormFields";

/** A `validate` for `TextInput`: a whole number within `min`..`max`. */
export function wholeNumber({ min = 1, max }: { min?: number; max?: number } = {}) {
  return (value: string): true | string => {
    const valid =
      /^\d+$/.test(value) && Number(value) >= min && (max === undefined || Number(value) <= max);
    if (valid) return true;
    return max === undefined
      ? `Enter a whole number, ${min} or more.`
      : `Enter a whole number from ${min} to ${max}.`;
  };
}

export const portNumber = wholeNumber({ min: 1, max: 65535 });

export const ID_FORMAT: TextFormat = {
  pattern: /^[a-z0-9][a-z0-9_-]*$/,
  message:
    "Use lower-case letters, digits, hyphens and underscores, starting with a letter or digit.",
  description: "Fixed once saved. Channels and the command line refer to it.",
};

/** An IPv4/IPv6 address or CIDR range, loosely: the engine parses it strictly. */
export const PEER_ADDRESS: TextFormat = {
  pattern: /^[0-9a-fA-F:.]+(\/\d{1,3})?$/,
  message: "Enter an IP address or a range such as 10.0.4.0/24.",
  description: "An IP address or CIDR range.",
};

/** Blank is allowed; otherwise a whole number of 1 or more. */
export function optionalWholeNumber(value: string): true | string {
  return value === "" ? true : wholeNumber()(value);
}

/** The pinned SFTP server key: `SHA256:` and the base64 of its digest, as `ssh-keygen -l` prints it. */
export const HOST_KEY_FINGERPRINT: TextFormat = {
  pattern: /^SHA256:[A-Za-z0-9+/]{43}=?$/,
  message: "Enter SHA256: followed by 43 base64 characters.",
  description:
    "The server's key is pinned: the connection is refused if the server shows a different one. Ask the server's administrator, or read it with ssh-keygen -lf. Looks like SHA256:uNiVztksCsDhcc0u9e8BujQXVUpKZIDTMczCvj3tD2s",
};

/** What is wrong with a column template, or null when it is a JSON object (spec §7.4). */
export function jsonObjectProblem(text: string): string | null {
  if (text.trim() === "") return "Enter the column template as a JSON object.";
  try {
    const parsed: unknown = JSON.parse(text);
    if (typeof parsed === "object" && parsed !== null && !Array.isArray(parsed)) return null;
    return "The template must be a JSON object, written with { }.";
  } catch (cause) {
    return `Not valid JSON: ${cause instanceof Error ? cause.message : "unreadable text"}.`;
  }
}

/** One glob over file names: `*` and `?` stand for any text and any one character. */
export const FILE_PATTERN: TextFormat = {
  pattern: /^[^/\\[\]{}]+$/,
  message:
    "Use only * and ? as wildcards, for example *.csv. A file name has no folders or brackets.",
  description: "Matches file names; `*` and `?` only, for example `*.csv`.",
};
