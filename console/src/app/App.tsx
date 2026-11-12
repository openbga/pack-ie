import { QueryClientProvider } from "@tanstack/react-query";
import { useState } from "react";
import { createHashRouter, RouterProvider } from "react-router";
import { createEngineApi, EngineProvider } from "@/api";
import { createQueryClient } from "@/api/hooks";
import { ToastProvider } from "@/components/Toast";
import { IdleProvider } from "@/lib/idle";
import { EngineGate } from "./EngineGate";
import { IdleOverlay } from "./IdleOverlay";
import { routes } from "./routes";

/**
 * Providers and routing. A hash router keeps every route inside the bundled `index.html`, so
 * the window never navigates to another document. The gate shows the "engine is not running"
 * screen instead of the app until an engine answers.
 */
export function App() {
  const [queryClient] = useState(createQueryClient);
  const [api] = useState(createEngineApi);
  const [router] = useState(() => createHashRouter(routes));
  return (
    <EngineProvider api={api}>
      <QueryClientProvider client={queryClient}>
        <ToastProvider>
          <IdleProvider>
            <EngineGate>
              <RouterProvider router={router} />
            </EngineGate>
            <IdleOverlay />
          </IdleProvider>
        </ToastProvider>
      </QueryClientProvider>
    </EngineProvider>
  );
}
