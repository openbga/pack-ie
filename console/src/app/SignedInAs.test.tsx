import { screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { EngineApi } from "@/api/engine";
import type { ConnectionStatus } from "@/api/types";
import { renderWithProviders } from "@/lib/test/render";
import { SignedInAs } from "./SignedInAs";

const connected = (actor: string | null): ConnectionStatus => ({
  state: "connected",
  endpoint: "/run/packie.sock",
  detail: null,
  since: "2026-11-06T12:00:00Z",
  actor,
});

const apiFor = (status: ConnectionStatus) =>
  ({ getConnection: async () => status }) as unknown as EngineApi;

describe("SignedInAs", () => {
  it("names the account the engine attributes the console's actions to", async () => {
    renderWithProviders(<SignedInAs />, { api: apiFor(connected("LAB\\jdoe")) });
    expect(await screen.findByText("Signed in as LAB\\jdoe")).toBeTruthy();
  });

  it("says nothing while the engine has not named the account", async () => {
    const { container } = renderWithProviders(<SignedInAs />, { api: apiFor(connected(null)) });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(container.textContent).toBe("");
  });
});
