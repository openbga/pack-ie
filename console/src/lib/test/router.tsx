import type { ReactNode } from "react";
import { createMemoryRouter, RouterProvider, useLocation } from "react-router";
import type { EngineApi } from "@/api/engine";
import { AriaRouter } from "@/lib/AriaRouter";
import { renderWithProviders } from "./render";

/**
 * Renders `routes` at `path` with the app's providers, react-aria links wired to the router
 * as in the app. Every other path renders its own address, so a test can check where a
 * screen navigated to.
 */
export function renderRoutes(
  routes: { path: string; element: ReactNode }[],
  path: string,
  options: { api?: EngineApi } = {},
) {
  const router = createMemoryRouter(
    [{ element: <AriaRouter />, children: [...routes, { path: "*", element: <LocationProbe /> }] }],
    { initialEntries: [path] },
  );
  return { ...renderWithProviders(<RouterProvider router={router} />, options), router };
}

function LocationProbe() {
  const location = useLocation();
  return (
    <p>
      Navigated to {location.pathname}
      {location.search}
    </p>
  );
}
