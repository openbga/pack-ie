import { act, screen, within } from "@testing-library/react";
import { vi } from "vitest";
import type { ChannelKind, TrafficSegment } from "@/api/types";
import { renderWithProviders } from "@/lib/test/render";
import { engineError, stubApi } from "../shared-operate/testFixtures";
import { TrafficPanel } from "./TrafficPanel";

const base64 = (...values: number[]) => btoa(String.fromCharCode(...values));
export const AT = "2026-11-06T14:03:05.123Z";

export function segment(
  seq: number,
  direction: TrafficSegment["direction"],
  ...values: number[]
): TrafficSegment {
  return { seq, at: AT, direction, base64: base64(...values) };
}

export function setup({
  kind = "bridge",
  subscribe,
  destinations,
}: {
  kind?: ChannelKind;
  subscribe?: () => never;
  destinations?: { destinationId: string; label: string }[];
} = {}) {
  let push: (segments: TrafficSegment[]) => void = () => undefined;
  const unsubscribe = vi.fn();
  const subscribeTraffic = vi.fn(
    (_id: string, onSegments: (segments: TrafficSegment[]) => void) => {
      if (subscribe) return subscribe();
      push = onSegments;
      return unsubscribe;
    },
  );
  const view = renderWithProviders(
    <TrafficPanel
      channelId="ch-alpha"
      kind={kind}
      endpointLabels={kind === "bridge" ? ["Analyzer COM3", "LIS listener"] : null}
      destinations={destinations}
    />,
    { api: stubApi({ subscribeTraffic }) },
  );
  return {
    ...view,
    subscribeTraffic,
    unsubscribe,
    push: (s: TrafficSegment[]) => act(() => push(s)),
  };
}

export const start = (user: ReturnType<typeof setup>["user"]) =>
  user.click(screen.getByRole("button", { name: "Start live transcript" }));

export const pane = (name: string) => screen.getByRole("log", { name: `Transcript ${name}` });
export const items = (name: string) => within(pane(name)).queryAllByRole("listitem");

/** The engine refuses a second transcript viewer. */
export const refuseSubscription = (): never => {
  throw engineError("conflict", "Transcript in use by another client");
};

/** Replaces the clipboard (after the user-event setup, which installs its own); returns what is written. */
export function stubClipboard() {
  const writeText = vi.fn(async () => undefined);
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  return writeText;
}

/** The text of the first copy. */
export const copiedText = (writeText: ReturnType<typeof stubClipboard>) =>
  String((writeText.mock.calls[0] as unknown[])[0]);
