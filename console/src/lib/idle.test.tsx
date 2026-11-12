import { act, render, screen } from "@testing-library/react";
import { type ReactNode, useRef } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_IDLE_MINUTES, IdleProvider, useIdle, useOnIdle } from "./idle";

function Listener({ onIdle, name = "listener" }: { onIdle: () => void; name?: string }) {
  useOnIdle(onIdle);
  return <p>{name}</p>;
}

/** Shows the idle flag and counts its own renders. */
function Status() {
  const { idle, resume } = useIdle();
  const renders = useRef(0);
  renders.current += 1;
  return (
    <>
      <p>{idle ? "idle" : "active"}</p>
      <p>renders {renders.current}</p>
      <button type="button" onClick={resume}>
        resume
      </button>
    </>
  );
}

const minutes = (count: number) => act(() => void vi.advanceTimersByTime(count * 60_000));

function renderIdle(children: ReactNode) {
  return render(<IdleProvider>{children}</IdleProvider>);
}

describe("idle protection", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.useFakeTimers();
  });
  afterEach(() => vi.useRealTimers());

  it("goes idle once the idle time has passed, and tells each listener once", () => {
    const first = vi.fn();
    const second = vi.fn();
    renderIdle(
      <>
        <Status />
        <Listener onIdle={first} />
        <Listener onIdle={second} />
      </>,
    );
    minutes(DEFAULT_IDLE_MINUTES - 1);
    expect(screen.getByText("active")).toBeTruthy();
    expect(first).not.toHaveBeenCalled();
    minutes(1.1);
    expect(screen.getByText("idle")).toBeTruthy();
    expect(first).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenCalledTimes(1);
    // Staying idle is not another idle period.
    minutes(60);
    expect(first).toHaveBeenCalledTimes(1);
  });

  it("goes idle again after Continue and a further idle time", () => {
    const onIdle = vi.fn();
    renderIdle(
      <>
        <Status />
        <Listener onIdle={onIdle} />
      </>,
    );
    minutes(16);
    act(() => screen.getByRole("button", { name: "resume" }).click());
    expect(screen.getByText("active")).toBeTruthy();
    minutes(14);
    expect(onIdle).toHaveBeenCalledTimes(1);
    minutes(2);
    expect(screen.getByText("idle")).toBeTruthy();
    expect(onIdle).toHaveBeenCalledTimes(2);
  });

  it("counts activity as presence, but only Continue ends an idle period", () => {
    renderIdle(<Status />);
    minutes(10);
    act(() => void document.dispatchEvent(new Event("keydown")));
    minutes(10);
    expect(screen.getByText("active")).toBeTruthy();
    minutes(6);
    expect(screen.getByText("idle")).toBeTruthy();
    act(() => void document.dispatchEvent(new Event("pointermove")));
    minutes(1);
    expect(screen.getByText("idle")).toBeTruthy();
  });

  it("does not render the app again when the pointer moves", () => {
    renderIdle(<Status />);
    const before = screen.getByText(/^renders /).textContent;
    for (let i = 0; i < 20; i++) act(() => void document.dispatchEvent(new Event("pointermove")));
    expect(screen.getByText(/^renders /).textContent).toBe(before);
  });

  it("calls the latest callback, and nobody after unmount", () => {
    const stale = vi.fn();
    const latest = vi.fn();
    const { rerender, unmount } = renderIdle(<Listener onIdle={stale} />);
    rerender(
      <IdleProvider>
        <Listener onIdle={latest} />
      </IdleProvider>,
    );
    minutes(16);
    expect(stale).not.toHaveBeenCalled();
    expect(latest).toHaveBeenCalledTimes(1);
    unmount();
    minutes(16);
    expect(latest).toHaveBeenCalledTimes(1);
  });

  it("tells the other listeners when one fails", () => {
    const report = vi.fn();
    vi.stubGlobal("reportError", report);
    const after = vi.fn();
    renderIdle(
      <>
        <Listener
          onIdle={() => {
            throw new Error("closing failed");
          }}
        />
        <Listener onIdle={after} />
      </>,
    );
    minutes(16);
    expect(after).toHaveBeenCalledTimes(1);
    expect(report).toHaveBeenCalledTimes(1);
    vi.unstubAllGlobals();
  });

  it("uses the stored idle time, and the default for a stored value that is not valid", () => {
    localStorage.setItem("packie.idleMinutes", "2");
    const { unmount } = renderIdle(<Status />);
    minutes(2.1);
    expect(screen.getByText("idle")).toBeTruthy();
    unmount();
    localStorage.setItem("packie.idleMinutes", "0");
    renderIdle(<Status />);
    minutes(2.1);
    expect(screen.getByText("active")).toBeTruthy();
  });
});
