import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { TauriBridge } from "./bridge";

/** The bridge to the real Tauri runtime; only available inside the console window. */
export const tauriBridge: TauriBridge = {
  invoke: <T>(command: string, args?: Record<string, unknown>) => invoke<T>(command, args),
  listen: <T>(event: string, handler: (payload: T) => void) =>
    listen<T>(event, (message) => handler(message.payload)),
};
