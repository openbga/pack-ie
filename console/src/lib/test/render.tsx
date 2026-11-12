import { QueryClientProvider } from "@tanstack/react-query";
import { render } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactElement } from "react";
import { EngineProvider } from "@/api/context";
import type { EngineApi } from "@/api/engine";
import { createQueryClient } from "@/api/hooks";
import { ToastProvider } from "@/components/Toast";

/**
 * Renders with the app's providers and a fresh cache; returns a `user` for events. Pass an
 * `api` (a `MockEngineApi` or a stub) for screens that read from the engine.
 */
export function renderWithProviders(ui: ReactElement, { api }: { api?: EngineApi } = {}) {
  const user = userEvent.setup();
  const content = (
    <QueryClientProvider client={createQueryClient()}>
      <ToastProvider>{ui}</ToastProvider>
    </QueryClientProvider>
  );
  const result = render(api ? <EngineProvider api={api}>{content}</EngineProvider> : content);
  return { ...result, user };
}
