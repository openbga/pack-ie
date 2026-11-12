import { act, screen, within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";

/** The data rows of the grid named `name`, or of every grid on the screen, without the header. */
export function gridRows(name?: string) {
  const scope = name ? within(screen.getByRole("grid", { name })) : screen;
  return scope.getAllByRole("row").slice(1);
}

/** The text of the cell in column `column` (from 0) of each data row, top to bottom. */
export const columnTexts = (column: number, name?: string) =>
  gridRows(name).map((row) => within(row).getAllByRole("gridcell")[column]?.textContent);

async function rowOfCell(name: string) {
  const cell = await screen.findByRole("gridcell", { name });
  const row = cell.closest<HTMLElement>("[role=row]");
  if (!row) throw new Error("no row");
  return { cell, row };
}

/** Selects the grid row holding a cell named `name`, by clicking its check cell. */
export async function selectRow(user: UserEvent, name: string) {
  const { row } = await rowOfCell(name);
  const [check] = within(row).getAllByRole("gridcell");
  if (!check) throw new Error("no check cell");
  await user.click(check);
}

/** Opens the grid row holding a cell named `name` with a single click on that cell. */
export async function openRow(user: UserEvent, name: string) {
  const { cell } = await rowOfCell(name);
  await user.click(cell);
}

/** Opens the grid row holding a cell named `name` with Enter, with focus on that cell. */
export async function openRowWithEnter(user: UserEvent, name: string) {
  const { cell } = await rowOfCell(name);
  act(() => cell.focus());
  await user.keyboard("{Enter}");
}

/**
 * Opens the actions menu of the row whose identifier is `identifier`, with its "⋯" button,
 * and returns the menu.
 */
export async function openRowActions(user: UserEvent, identifier: string) {
  await user.click(await screen.findByRole("button", { name: `Actions for ${identifier}` }));
  return screen.findByRole("menu");
}
