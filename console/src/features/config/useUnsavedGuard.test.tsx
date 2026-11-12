import { act, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { renderRoutes } from "@/lib/test/router";
import { useUnsavedGuard } from "./useUnsavedGuard";

// The window API of the desktop shell, which does not exist outside it.
const windowApi = vi.hoisted(() => {
  const state: {
    handler: ((event: { preventDefault: () => void }) => void) | null;
    unlisten: ReturnType<typeof vi.fn>;
    destroy: ReturnType<typeof vi.fn>;
    onCloseRequested: ReturnType<typeof vi.fn>;
  } = {
    handler: null,
    unlisten: vi.fn(),
    destroy: vi.fn(async () => undefined),
    onCloseRequested: vi.fn(),
  };
  return state;
});

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    onCloseRequested: windowApi.onCloseRequested,
    destroy: windowApi.destroy,
  }),
}));

let allowLeave: () => void = () => undefined;

function Page({ isDirty }: { isDirty: boolean }) {
  const guard = useUnsavedGuard(isDirty);
  allowLeave = guard.allowLeave;
  return <div>{guard.dialog}</div>;
}

const render = (isDirty: boolean) =>
  renderRoutes([{ path: "/", element: <Page isDirty={isDirty} /> }], "/");

function leave() {
  const event = new Event("beforeunload", { cancelable: true });
  window.dispatchEvent(event);
  return event;
}

beforeEach(() => {
  windowApi.handler = null;
  windowApi.unlisten = vi.fn();
  windowApi.destroy = vi.fn(async () => undefined);
  windowApi.onCloseRequested = vi.fn(async (handler) => {
    windowApi.handler = handler;
    return windowApi.unlisten;
  });
});

afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
});

describe("useUnsavedGuard: reload and close in a browser", () => {
  it("asks the browser to confirm while the form has unsaved input", () => {
    render(true);
    expect(leave().defaultPrevented).toBe(true);
  });

  it("lets a clean form go, and lets a deliberate exit go after a save", () => {
    const { unmount } = render(false);
    expect(leave().defaultPrevented).toBe(false);
    unmount();
    render(true);
    act(() => allowLeave());
    expect(leave().defaultPrevented).toBe(false);
  });

  it("stops asking once the page is gone", () => {
    const { unmount } = render(true);
    unmount();
    expect(leave().defaultPrevented).toBe(false);
  });

  it("does not use the desktop window outside the desktop console", () => {
    render(true);
    expect(windowApi.onCloseRequested).not.toHaveBeenCalled();
  });
});

describe("useUnsavedGuard: closing the desktop window", () => {
  beforeEach(() => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
  });

  function closeRequest() {
    const preventDefault = vi.fn();
    act(() => windowApi.handler?.({ preventDefault }));
    return preventDefault;
  }

  it("holds a close request while the form is dirty and asks first", async () => {
    const { user } = render(true);
    await waitFor(() => expect(windowApi.handler).not.toBeNull());
    const preventDefault = closeRequest();
    expect(preventDefault).toHaveBeenCalled();
    const dialog = await screen.findByRole("alertdialog", {
      name: "Discard unsaved changes and close?",
    });
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(windowApi.destroy).not.toHaveBeenCalled();
  });

  it("closes the window when the operator discards the changes", async () => {
    const { user } = render(true);
    await waitFor(() => expect(windowApi.handler).not.toBeNull());
    closeRequest();
    const dialog = await screen.findByRole("alertdialog", {
      name: "Discard unsaved changes and close?",
    });
    await user.click(within(dialog).getByRole("button", { name: "Discard and close" }));
    expect(windowApi.destroy).toHaveBeenCalledTimes(1);
  });

  it("does not listen while the form is clean, and stops listening when the page goes", async () => {
    const clean = render(false);
    expect(windowApi.onCloseRequested).not.toHaveBeenCalled();
    clean.unmount();
    const dirty = render(true);
    await waitFor(() => expect(windowApi.handler).not.toBeNull());
    dirty.unmount();
    expect(windowApi.unlisten).toHaveBeenCalled();
  });

  it("lets a close through after a save", async () => {
    render(true);
    await waitFor(() => expect(windowApi.handler).not.toBeNull());
    act(() => allowLeave());
    const preventDefault = closeRequest();
    expect(preventDefault).not.toHaveBeenCalled();
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });
});
