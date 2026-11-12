import type { Event, IpcError } from "@/ipc/generated";

/**
 * The only door to Tauri. `IpcEngineApi` gets one through its constructor; the real one is
 * `tauriBridge.ts`, tests pass a fake.
 */
export interface TauriBridge {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  /** Resolves with the function that stops listening. */
  listen<T>(event: string, handler: (payload: T) => void): Promise<() => void>;
}

/** What a rejected command carries (src-tauri/src/error.rs). */
export type CommandError =
  | { kind: "ipc"; error: IpcError }
  | { kind: "disconnected"; message: string }
  | { kind: "internal"; message: string };

interface EngineIdentity {
  engine_version: string;
  protocol_version: number;
  node_id: string;
  node_name: string;
  /** The OS account the engine attributes this connection's actions to. */
  actor?: { name: string } | null;
}

/** `packie://connection` payload and the `ipc_connection` result. */
export interface ConnectionPayload {
  state: "connected" | "disconnected" | "reconnecting";
  endpoint: string | null;
  error: string | null;
  attempt: number;
  engine: EngineIdentity | null;
  since: string;
}

/** `packie://event` payload. */
export interface EventPayload {
  subscription: number;
  event: Event;
}

export const EVENT_CHANNEL = "packie://event";
export const CONNECTION_CHANNEL = "packie://connection";
