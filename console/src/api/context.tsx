import { createContext, type ReactNode, useContext } from "react";
import type { EngineApi } from "./engine";

const EngineContext = createContext<EngineApi | null>(null);

/** Provides the engine the pages talk to: the IPC client in the desktop app, the mock in the browser. */
export function EngineProvider({ api, children }: { api: EngineApi; children: ReactNode }) {
  return <EngineContext.Provider value={api}>{children}</EngineContext.Provider>;
}

export function useEngineApi(): EngineApi {
  const api = useContext(EngineContext);
  if (api === null) throw new Error("useEngineApi needs an EngineProvider");
  return api;
}
