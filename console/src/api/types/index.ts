/**
 * The console's view of the engine: every DTO that crosses the `EngineApi` boundary.
 *
 * Modelled on docs/architecture.md §2 and §6. Vocabulary that is identical to the engine's
 * (the enums) is re-exported from the ts-rs output in `src/ipc/generated/`; the rest is the
 * shape the pages want (camelCase, resolved labels), produced from the generated DTOs by
 * `src/api/ipc/`. Timestamps are RFC 3339 strings; ids are opaque strings.
 */

export type * from "./audit";
export type * from "./config";
export type * from "./core";
export type * from "./logs";
export type * from "./messages";
export type * from "./setup";
export type * from "./snapshot";
