import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { mockGridLayout } from "@/lib/test/layout";
import { renderWithProviders } from "@/lib/test/render";
import { stubApi } from "../shared-operate/testFixtures";
import { TrafficPanel } from "./TrafficPanel";
import {
  AT,
  copiedText,
  items,
  pane,
  refuseSubscription,
  segment,
  setup,
  start,
  stubClipboard,
} from "./trafficTestFixtures";

beforeEach(() => mockGridLayout());

describe("TrafficPanel", () => {
  it("is off by default, says bodies are shown only here and never stored, and does not subscribe", async () => {
    const { subscribeTraffic, container } = setup();
    expect(screen.getByText(/The transcript is off/).textContent).toContain(
      "never stored or logged",
    );
    expect(subscribeTraffic).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Start live transcript" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Freeze" }).hasAttribute("disabled")).toBe(true);
    await expectNoA11yViolations(container);
  });

  it("subscribes on Start and shows two panes, each segment timestamped with its text", async () => {
    const { user, subscribeTraffic, push, container } = setup();
    await start(user);
    expect(subscribeTraffic).toHaveBeenCalledWith(
      "ch-alpha",
      expect.any(Function),
      expect.any(Function),
    );
    expect(screen.getAllByText("Waiting for traffic…")).toHaveLength(2);

    push([
      segment(1, "a_to_b", 0x02, 0x31, 0x48, 0x7c, 0x0d, 0x0a, 0x03),
      segment(2, "b_to_a", 0x06),
      segment(3, "a_to_b", 0x04),
    ]);
    expect(items("A → B")).toHaveLength(2);
    expect(items("B → A")).toHaveLength(1);
    expect(items("A → B")[0]?.textContent).toContain("<STX>1H|<CR><LF><ETX>");
    expect(items("A → B")[0]?.querySelector("time")?.getAttribute("datetime")).toBe(AT);
    expect(items("B → A")[0]?.textContent).toContain("<ACK>");
    expect(screen.getByRole("heading", { name: "A → B" })).toBeTruthy();
    expect(screen.getByRole("heading", { name: "B → A" })).toBeTruthy();
    expect(screen.getByText("A = Analyzer COM3 · B = LIS listener")).toBeTruthy();
    await expectNoA11yViolations(container);
  });

  it("shows text only until the hex toggle is on", async () => {
    const { user, push } = setup();
    await start(user);
    push([segment(1, "a_to_b", 0x02, 0x31, 0x03)]);
    expect(items("A → B")[0]?.textContent).not.toContain("02 31 03");
    await user.click(screen.getByRole("checkbox", { name: "Show hex" }));
    expect(items("A → B")[0]?.textContent).toContain("02 31 03");
    await user.click(screen.getByRole("checkbox", { name: "Show hex" }));
    expect(items("A → B")[0]?.textContent).not.toContain("02 31 03");
  });

  it("names the panes Source and Destinations for a message channel", async () => {
    const { user, push } = setup({ kind: "message" });
    await start(user);
    push([segment(1, "a_to_b", 0x41), segment(2, "b_to_a", 0x42)]);
    expect(items("Source")[0]?.textContent).toContain("A");
    expect(items("Destinations")[0]?.textContent).toContain("B");
    expect(screen.queryByText(/A = /)).toBeNull();
  });

  it("freezing stops the display while capture continues; unfreezing shows the newest", async () => {
    const { user, push } = setup();
    await start(user);
    push([segment(1, "a_to_b", 0x41)]);
    await user.click(screen.getByRole("button", { name: "Freeze" }));
    push([segment(2, "a_to_b", 0x42), segment(3, "b_to_a", 0x43)]);

    expect(items("A → B")).toHaveLength(1);
    expect(items("B → A")).toHaveLength(0);
    expect(screen.getByText("Display frozen; 2 new segments captured.")).toBeTruthy();

    push([segment(4, "b_to_a", 0x44)]);
    expect(screen.getByText("Display frozen; 3 new segments captured.")).toBeTruthy();

    await user.click(screen.getByRole("button", { name: "Unfreeze" }));
    expect(items("A → B")).toHaveLength(2);
    expect(items("B → A")).toHaveLength(2);
    expect(screen.queryByText(/Display frozen/)).toBeNull();
  });

  it("unsubscribes on Stop, keeps what was captured, and Clear empties it", async () => {
    const { user, unsubscribe, push } = setup();
    await start(user);
    push([segment(1, "a_to_b", 0x41)]);
    await user.click(screen.getByRole("button", { name: "Stop live transcript" }));
    expect(unsubscribe).toHaveBeenCalledOnce();
    expect(screen.getByText(/The transcript is off/)).toBeTruthy();
    expect(items("A → B")).toHaveLength(1);

    await user.click(screen.getByRole("button", { name: "Clear" }));
    expect(items("A → B")).toHaveLength(0);
  });

  it("unsubscribes when the screen goes away", async () => {
    const { user, unsubscribe, unmount } = setup();
    await start(user);
    unmount();
    expect(unsubscribe).toHaveBeenCalledOnce();
  });

  it("keeps only the newest 1000 segments in each pane and says how many it received", async () => {
    const { user, push } = setup();
    await start(user);
    push(
      Array.from({ length: 1200 }, (_, index) => segment(index + 1, "a_to_b", 0x41 + (index % 26))),
    );
    push([segment(2000, "b_to_a", 0x41)]);
    expect(items("A → B")).toHaveLength(1000);
    expect(items("B → A")).toHaveLength(1);
    expect(screen.getByText("Newest 1,000 of 1,200 segments")).toBeTruthy();
  });

  it("shows the engine's words when it refuses the subscription, and stays off", async () => {
    const { user, container } = setup({
      subscribe: refuseSubscription,
    });
    await start(user);
    expect(await screen.findByText("Transcript in use by another client")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Start live transcript" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Stop live transcript" })).toBeNull();
    await expectNoA11yViolations(container);
  });

  it("clears an earlier refusal when the operator tries again", async () => {
    let refuse = true;
    const subscribeTraffic = vi.fn(() => {
      if (refuse) refuseSubscription();
      return () => undefined;
    });
    const { user } = renderWithProviders(
      <TrafficPanel channelId="ch-alpha" kind="bridge" endpointLabels={null} />,
      { api: stubApi({ subscribeTraffic }) },
    );
    await start(user);
    expect(await screen.findByText("Transcript in use by another client")).toBeTruthy();
    refuse = false;
    await start(user);
    await waitFor(() =>
      expect(screen.queryByText("Transcript in use by another client")).toBeNull(),
    );
    expect(screen.getByRole("button", { name: "Stop live transcript" })).toBeTruthy();
  });

  it("stops following in a pane when the operator scrolls up, and follows again on request", async () => {
    const { user, push } = setup();
    await start(user);
    push([segment(1, "a_to_b", 0x41)]);
    const autoscroll = screen.getByRole("checkbox", {
      name: "Autoscroll A → B",
    }) as HTMLInputElement;
    expect(autoscroll.checked).toBe(true);

    // The mocked layout is 10,000,000px tall and 320px visible; this scroll position is far from the end.
    const log = pane("A → B");
    log.scrollTop = 0;
    fireEvent.scroll(log);
    await waitFor(() => expect(autoscroll.checked).toBe(false));
    expect(
      (screen.getByRole("checkbox", { name: "Autoscroll B → A" }) as HTMLInputElement).checked,
    ).toBe(true);

    await user.click(autoscroll);
    expect(autoscroll.checked).toBe(true);
  });

  it("copies both panes as text", async () => {
    const { user, push } = setup();
    const writeText = stubClipboard();
    await start(user);
    push([segment(1, "a_to_b", 0x05), segment(2, "b_to_a", 0x06)]);
    await user.click(screen.getByRole("button", { name: "Copy" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledOnce());
    const text = copiedText(writeText);
    expect(text).toContain("== A → B ==");
    expect(text).toContain("<ENQ>");
    expect(text).toContain("== B → A ==");
    expect(text).toContain("<ACK>");
    expect(await screen.findByText("Copied 2 segments")).toBeTruthy();
  });
});
