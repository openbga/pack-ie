import type { Page } from "@playwright/test";
import { main } from "./support";

/** A named region of the page's main area. */
export const region = (page: Page, name: string) =>
  main(page).getByRole("region", { name, exact: true });

/** A count as the pages show it: digits, thousands separators allowed. */
export const COUNT = /^[\d,]+$/;
