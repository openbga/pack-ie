import { act, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { EngineApi } from "@/api";
import type { ConnectionStatus } from "@/api/types";
import { renderWithProviders } from "@/lib/test/render";
import { EngineGate } from "./EngineGate";

const status = (overrides: Partial<ConnectionStatus> = {}): ConnectionStatus => ({
  state: "connected",
  endpoint: "/run/packie.sock",
  detail: null,
  since: "2026-11-06T12:00:00Z",
  actor: "LAB\\jdoe",
  ...overrides,
});

/** An engine whose connection state the test drives. */
function fakeEngine(initial: ConnectionStatus, startService = vi.fn(async () => "started")) {
  let listener: ((status: ConnectionStatus) => void) | null = null;
  const api = {
    getConnection: async () => initial,
    subscribeConnection: (onStatus: (status: ConnectionStatus) => void) => {
      listener = onStatus;
      return () => {
        listener = null;
      };
    },
    startService,
  } as unknown as EngineApi;
  return { api, push: (next: ConnectionStatus) => act(() => listener?.(next)) };
}

describe("EngineGate", () => {
  it("shows the app once an engine answers", async () => {
    const { api } = fakeEngine(status());
    renderWithProviders(
      <EngineGate>
        <p>the app</p>
      </EngineGate>,
      { api },
    );
    expect(await screen.findByText("the app")).toBeTruthy();
  });

  it("says where it looked and why, and offers to start the service", async () => {
    const { api } = fakeEngine(
      status({ state: "disconnected", detail: "connection refused" }),
      vi.fn(async () => {
        throw {
          code: "unavailable",
          message: "`packie service start` is not implemented.",
          fieldErrors: [],
          correlationId: "",
        };
      }),
    );
    const { user } = renderWithProviders(
      <EngineGate>
        <p>the app</p>
      </EngineGate>,
      { api },
    );
    expect(await screen.findByText("The engine is not running")).toBeTruthy();
    expect(
      screen.getByText(/No engine answered at \/run\/packie\.sock\. connection refused/),
    ).toBeTruthy();
    expect(screen.queryByText("the app")).toBeNull();
    // The service manager needs administrator rights: the page says so and shows the command.
    expect(screen.getByText(/administrator terminal/)).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Start the service" }));
    // The refusal is shown as the engine's own words, not as a generic failure.
    expect(await screen.findByText("`packie service start` is not implemented.")).toBeTruthy();
  });

  it("continues by itself when the engine appears", async () => {
    const { api, push } = fakeEngine(status({ state: "disconnected" }));
    renderWithProviders(
      <EngineGate>
        <p>the app</p>
      </EngineGate>,
      { api },
    );
    await screen.findByText("The engine is not running");
    await push(status());
    expect(await screen.findByText("the app")).toBeTruthy();
  });

  it("keeps the pages and shows a banner when the connection is lost", async () => {
    const { api, push } = fakeEngine(status());
    renderWithProviders(
      <EngineGate>
        <p>the app</p>
      </EngineGate>,
      { api },
    );
    await screen.findByText("the app");
    await push(status({ state: "reconnecting", detail: "connection refused" }));
    expect(await screen.findByText("Lost the connection to the engine")).toBeTruthy();
    expect(screen.getByText("the app")).toBeTruthy();
    await push(status());
    await waitFor(() => expect(screen.queryByText("Lost the connection to the engine")).toBeNull());
  });
});
