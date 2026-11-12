import { screen } from "@testing-library/react";

/** Whether one of the screen's status regions (live announcements) shows `text`. */
export const status = (text: string) =>
  screen.getAllByRole("status").some((element) => element.textContent?.includes(text));
