import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Countdown } from "./Countdown";

describe("Countdown", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-11-06T12:00:00.000Z"));
  });
  afterEach(() => vi.useRealTimers());

  it("ticks down every second and ends at now", () => {
    render(<Countdown until="2026-11-06T12:00:05.000Z" />);
    expect(screen.getByText("in 5 s")).toBeTruthy();
    act(() => vi.advanceTimersByTime(1000));
    expect(screen.getByText("in 4 s")).toBeTruthy();
    act(() => vi.advanceTimersByTime(3000));
    expect(screen.getByText("in 1 s")).toBeTruthy();
    act(() => vi.advanceTimersByTime(1000));
    expect(screen.getByText("now")).toBeTruthy();
  });

  it("stops its timer when it goes away", () => {
    const { unmount } = render(<Countdown until="2026-11-06T12:00:05.000Z" />);
    expect(vi.getTimerCount()).toBe(1);
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  });
});
