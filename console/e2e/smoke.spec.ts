import { expect, test } from "@playwright/test";
import { loaded, open } from "./support";

test("the console opens on the dashboard against the mock engine", async ({ page }) => {
  await open(page, "/dashboard");
  await loaded(page, "Dashboard");
  await expect(page.getByRole("navigation", { name: "Main" })).toBeVisible();
});
