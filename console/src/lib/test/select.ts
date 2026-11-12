import { screen } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";

/** Opens the select whose trigger is labelled `label` and clicks `option`. */
export async function chooseOption(user: UserEvent, label: string | RegExp, option: string) {
  await user.click(screen.getByRole("button", { name: label }));
  await user.click(await screen.findByRole("option", { name: option }));
}
