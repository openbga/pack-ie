import { act, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { MockEngineApi } from "@/api/mock";
import type { ChannelSnapshot } from "@/api/types";
import { mockGridLayout } from "@/lib/test/layout";
import { renderRoutes } from "@/lib/test/router";
import { channelRoutes } from "./testFixtures";

/** The channel page against the mock engine, for what each kind of channel reports. */
describe("Channel page against the mock engine", () => {
  let api: MockEngineApi;
  beforeEach(() => {
    mockGridLayout();
    api = new MockEngineApi({ latency: "none", messageCount: 50, logRingSize: 300 });
  });
  afterEach(() => api.dispose());

  const find = async (predicate: (channel: ChannelSnapshot) => boolean) => {
    const channel = (await api.getSnapshot()).channels.find(predicate);
    if (!channel) throw new Error("no such channel in the mock");
    return channel;
  };

  it("shows a running bridge with both sides, both directions and recent events", async () => {
    const bridge = await find((c) => c.kind === "bridge" && c.state === "running");
    renderRoutes(channelRoutes, `/channels/${bridge.id}`, { api });
    expect(await screen.findByRole("heading", { level: 1, name: bridge.label })).toBeTruthy();
    const endpoints = await screen.findByRole("table", { name: "Endpoints of this channel" });
    expect(within(endpoints).getAllByRole("row")).toHaveLength(3);
    expect(screen.getByRole("table", { name: "Bytes by direction" })).toBeTruthy();
    expect(
      await screen.findByRole("table", { name: "Recent events of this channel" }),
    ).toBeTruthy();
    expect(document.body.textContent).not.toMatch(/undefined|NaN|\[object/);
  });

  it("shows a message channel's source and destinations", async () => {
    const message = await find((c) => c.kind === "message" && c.state === "running");
    renderRoutes(channelRoutes, `/channels/${message.id}`, { api });
    expect(await screen.findByRole("heading", { level: 2, name: "Source connector" })).toBeTruthy();
    expect(await screen.findAllByRole("region", { name: /^Destination / })).not.toHaveLength(0);
    expect(document.body.textContent).not.toMatch(/undefined|NaN|\[object/);
  });

  it("streams a transcript in both panes and refuses a second viewer in the engine's words", async () => {
    const bridge = await find((c) => c.kind === "bridge" && c.state === "running");
    const { user } = renderRoutes(channelRoutes, `/channels/${bridge.id}`, { api });
    await screen.findByRole("heading", { level: 1, name: bridge.label });

    // Another client takes the transcript first.
    const unsubscribe = api.subscribeTraffic(bridge.id, () => undefined);
    await user.click(screen.getByRole("button", { name: "Start live transcript" }));
    expect(await screen.findByText("Transcript in use by another client")).toBeTruthy();
    act(() => unsubscribe());

    await user.click(screen.getByRole("button", { name: "Start live transcript" }));
    await waitFor(
      () => {
        const forward = within(screen.getByRole("log", { name: "Transcript A → B" }));
        expect(forward.queryAllByRole("listitem").length).toBeGreaterThan(0);
      },
      { timeout: 8000 },
    );
    await user.click(screen.getByRole("button", { name: "Stop live transcript" }));
  }, 15_000);

  it("streams a message channel's transcript into Source and Destinations, tagged by destination", async () => {
    const message = await find((c) => c.kind === "message" && c.state === "running");
    const detail = await api.getChannel(message.id);
    const { user } = renderRoutes(channelRoutes, `/channels/${message.id}`, { api });
    await screen.findByRole("heading", { level: 1, name: message.label });
    await user.click(screen.getByRole("button", { name: "Start live transcript" }));
    await waitFor(
      () => {
        const source = within(screen.getByRole("log", { name: "Transcript Source" }));
        const sent = within(screen.getByRole("log", { name: "Transcript Destinations" }));
        expect(source.queryAllByRole("listitem").length).toBeGreaterThan(0);
        expect(sent.queryAllByRole("listitem").length).toBeGreaterThan(0);
      },
      { timeout: 8000 },
    );
    const destinations = screen.getByRole("log", { name: "Transcript Destinations" });
    const labels = detail.destinations.map((d) => d.label);
    expect(labels.some((label) => destinations.textContent?.includes(label))).toBe(true);
    expect(screen.queryByText(/does not tag Destinations segments/)).toBeNull();
    expect(destinations.textContent).toContain("→ Sent");
    expect(screen.getByRole("log", { name: "Transcript Source" }).textContent).toContain(
      "← Received",
    );
    await user.click(screen.getByRole("button", { name: "Stop live transcript" }));
  }, 15_000);
});
