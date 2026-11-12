import { within } from "@testing-library/react";
import { columnTexts, gridRows } from "@/lib/test/grid";
import { renderRoutes } from "@/lib/test/router";
import { connection, type snapshot, stubApi } from "../shared-operate/testFixtures";
import { DashboardPage } from "./DashboardPage";

const routes = [{ path: "/dashboard", element: <DashboardPage /> }];

export function renderDashboard(
  getSnapshot: () => Promise<ReturnType<typeof snapshot>>,
  getConnection = async () => connection(),
) {
  return renderRoutes(routes, "/dashboard", { api: stubApi({ getSnapshot, getConnection }) });
}

export const rows = () => gridRows("Channels");
export const cellsOf = (row: HTMLElement) => within(row).getAllByRole("gridcell");
export const labels = () =>
  columnTexts(0, "Channels").map((text) => text?.replace(/ (Bridge|Message)$/, ""));
export const rowOf = (name: string) => {
  const row = rows().find((each) => cellsOf(each)[0]?.textContent?.startsWith(name));
  if (!row) throw new Error(`no row for ${name}`);
  return row;
};
