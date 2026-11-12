/** The parts of an engine error summary that the detail view lists on their own. */
export interface ErrorParts {
  /** `A`, `B`, `source` or `destination <id>`. */
  role: string;
  endpointLabel: string;
  /** `connect`, `listen`, `accept`, `open`, `read`, `write`, `send` and so on. */
  operation: string;
  cause: string;
}

const SUMMARY = /^Channel \S+ (A|B|source|destination \S+) endpoint "([^"]*)" (\w+) failed: (.+)$/s;

/**
 * Splits a summary of the engine's one error pattern (docs/spec/diagnostics.md section 7.4:
 * `Channel {id} {role} endpoint "{label}" {operation} failed: {cause}. {next action}.`).
 * Returns null for any other wording, which the console then shows whole, never guessed at.
 */
export function parseErrorSummary(summary: string): ErrorParts | null {
  const match = SUMMARY.exec(summary.trim());
  if (match === null) return null;
  const [, role, endpointLabel, operation, rest = ""] = match;
  if (role === undefined || endpointLabel === undefined || operation === undefined) return null;
  const sentenceEnd = rest.indexOf(". ");
  const cause = (sentenceEnd === -1 ? rest : rest.slice(0, sentenceEnd)).replace(/\.$/, "");
  return { role, endpointLabel, operation, cause };
}
