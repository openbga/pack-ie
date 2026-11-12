import { screen, within } from "@testing-library/react";
import { vi } from "vitest";
import type { DeliveryState, MessageState, ResendResult } from "@/api/types";
import { renderWithProviders } from "@/lib/test/render";
import { stubApi } from "./fixtures";
import { ResendDialog } from "./ResendDialog";
import type { ResendSubject } from "./resendRules";

export const accepted = (
  id: string,
  label: string,
  delivered = true,
): ResendResult["destinations"][number] => ({
  destinationId: id,
  destinationLabel: label,
  deliveryId: delivered ? `q-${id}` : null,
  rejected: null,
});

export function subject(
  destinations: [string, DeliveryState][],
  state: MessageState = "routed",
): ResendSubject {
  return {
    messageId: "msg-000001",
    identifier: "CTRL-000001",
    channelLabel: "Synthetic chemistry",
    state,
    destinations: destinations.map(([label, deliveryState]) => ({
      id: `dest-${label}`,
      label,
      state: deliveryState,
    })),
  };
}

export function open(s: ResendSubject, api = stubApi(), onClose = vi.fn(), initial?: string[]) {
  const rendered = renderWithProviders(
    <ResendDialog subject={s} onClose={onClose} initialDestinations={initial} />,
    { api },
  );
  return { ...rendered, api, onClose };
}

export const dialog = () => screen.getByRole("dialog", { name: "Resend CTRL-000001" });
export const box = (name: string | RegExp) =>
  within(dialog()).getByRole("checkbox", { name }) as HTMLInputElement;
export const submit = (user: ReturnType<typeof renderWithProviders>["user"], name = "Resend") =>
  user.click(within(dialog()).getByRole("button", { name }));
