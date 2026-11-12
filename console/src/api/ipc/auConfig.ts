/**
 * AU instrument settings (docs/spec/configuration.md section 7.2) and the console's `AuSettings`.
 * There are no defaults: settings that are not all there read as "not set up" (undefined), and
 * writing overwrites only the fields the operator changed.
 */

import type { AuFormat, AuFraming, AuSettings as EngineAuSettings } from "@/ipc/generated";
import type { AuSettings } from "../types";
import { isEqual, isRecord } from "./util";
import type { WireSettings } from "./wire";

/** Where a field lives in the engine's AU settings, named by the generated type's own keys. */
type EnginePath =
  | readonly [Exclude<keyof EngineAuSettings, "framing" | "format">]
  | readonly ["framing", keyof AuFraming]
  | readonly ["format", keyof AuFormat];

/** Each modeled field and where it lives in the engine's settings object. */
const FIELDS: [keyof AuSettings, EnginePath][] = [
  ["profile", ["profile"]],
  ["framingStart", ["framing", "start"]],
  ["framingEnd", ["framing", "end"]],
  ["bcc", ["framing", "bcc"]],
  ["useEtb", ["framing", "use_etb"]],
  ["maxWireLen", ["framing", "max_wire_len"]],
  ["framingClass", ["framing", "class"]],
  ["unitNo", ["format", "unit_no"]],
  ["rackDigits", ["format", "rack_digits"]],
  ["sampleIdDigits", ["format", "sample_id_digits"]],
  ["sex", ["format", "sex"]],
  ["age", ["format", "age"]],
  ["month", ["format", "month"]],
  ["patientInfoDigits", ["format", "patient_info_digits"]],
  ["resultDigits", ["format", "result_digits"]],
  ["frameTimeoutMs", ["frame_timeout_ms"]],
];

function valueAt(settings: WireSettings, path: readonly string[]): unknown {
  let at: unknown = settings;
  for (const key of path) {
    if (!isRecord(at)) return undefined;
    at = at[key];
  }
  return at;
}

function setAt(settings: WireSettings, path: readonly string[], value: unknown): void {
  let at = settings;
  for (const key of path.slice(0, -1)) {
    const next = at[key];
    if (isRecord(next)) at = next;
    else {
      const made: WireSettings = {};
      at[key] = made;
      at = made;
    }
  }
  const last = path.at(-1);
  if (last !== undefined) at[last] = Array.isArray(value) ? [...value] : value;
}

const TEST_CODE_DIGITS = ["format", "test_code_digits"] as const;
const BLOCK_TIMEOUT = ["block_timeout_ms"] as const;
/** The engine's default block timeout, which an absent `block_timeout_ms` means. */
const DEFAULT_BLOCK_TIMEOUT_MS = 9900;

/** The AU settings of an engine settings object, or undefined when they are not all there. */
export function auFromWire(settings: WireSettings | undefined): AuSettings | undefined {
  if (settings === undefined) return undefined;
  const au: Record<string, unknown> = {};
  for (const [field, path] of FIELDS) {
    const value = valueAt(settings, path);
    if (value === undefined || value === null) return undefined;
    au[field] = Array.isArray(value) ? [...value] : value;
  }
  // The test code width is optional (the engine's default is 2), so it never makes the sheet "not set up".
  const digits = valueAt(settings, TEST_CODE_DIGITS);
  if (digits === 2 || digits === 3) au.testCodeDigits = digits;
  // The block timeout is optional too (the engine's default is 9900); read it when it is there.
  const block = valueAt(settings, BLOCK_TIMEOUT);
  if (typeof block === "number") au.blockTimeoutMs = block;
  return au as unknown as AuSettings;
}

/**
 * Writes the AU fields of `ui` into `settings`: all of them when `base` is undefined, else
 * those that differ from `base`.
 */
export function auToWire(
  settings: WireSettings,
  ui: AuSettings,
  base: AuSettings | undefined,
): void {
  for (const [field, path] of FIELDS) {
    if (base === undefined || !isEqual(ui[field], base[field])) setAt(settings, path, ui[field]);
  }
  const digits = ui.testCodeDigits ?? 2;
  if (
    digits !== (base?.testCodeDigits ?? 2) ||
    (base === undefined && ui.testCodeDigits !== undefined)
  ) {
    setAt(settings, TEST_CODE_DIGITS, digits);
  }
  // Written only when the operator set it: a value on a sheet with no base, or one that differs
  // from what the document means now (its value, else the default). An unset timeout is never
  // filled in with the default.
  const block = ui.blockTimeoutMs ?? DEFAULT_BLOCK_TIMEOUT_MS;
  const changed = block !== (base?.blockTimeoutMs ?? DEFAULT_BLOCK_TIMEOUT_MS);
  if (changed || (base === undefined && ui.blockTimeoutMs !== undefined)) {
    setAt(settings, BLOCK_TIMEOUT, block);
  }
}
