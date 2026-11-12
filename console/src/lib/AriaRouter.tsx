import { RouterProvider } from "react-aria-components";
import { type NavigateOptions, Outlet, useHref, useNavigate } from "react-router";

declare module "react-aria-components" {
  interface RouterConfig {
    routerOptions: NavigateOptions;
  }
}

/**
 * Lets react-aria links, tabs, breadcrumbs and menu items navigate inside the router, with
 * react-router's navigation options (such as `state`) as their `routerOptions`. The root
 * route's element in the app (`app/routes.tsx`) and in screen tests (`lib/test/router`).
 */
export function AriaRouter() {
  const navigate = useNavigate();
  return (
    <RouterProvider navigate={(path, options) => void navigate(path, options)} useHref={useHref}>
      <Outlet />
    </RouterProvider>
  );
}
