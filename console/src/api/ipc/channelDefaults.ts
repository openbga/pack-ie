// The specification's defaults for channel documents (docs/spec/configuration.md section 6).
export const DEFAULT_BUFFER_BYTES = 10 * 1024 * 1024;
export const DEFAULT_INITIAL_MS = 1000;
export const DEFAULT_MAX_MS = 60_000;
export const DEFAULT_ATTEMPTS = 5;
export const DEFAULT_RETENTION_DAYS = 90;
/** What a bridge tap keeps of the messages it records (a tap has its own, shorter default). */
export const DEFAULT_TAP_RETENTION_DAYS = 30;
