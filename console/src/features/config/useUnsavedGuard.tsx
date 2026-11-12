import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useRef, useState } from "react";
import { useBlocker } from "react-router";
import { ConfirmDialog } from "@/components/ConfirmDialog";

/** True inside the desktop console's window, where the native close request exists. */
function inDesktopWindow(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * Warns before the browser reloads or closes the page (the browser shows its own prompt).
 * `allowed` lets a deliberate exit, such as after a save, through.
 */
function useBeforeUnloadGuard(isDirty: boolean, allowed: { current: boolean }) {
  useEffect(() => {
    if (!isDirty) return;
    const warn = (event: BeforeUnloadEvent) => {
      if (allowed.current) return;
      event.preventDefault();
      // Some browsers only prompt when a return value is set.
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", warn);
    return () => window.removeEventListener("beforeunload", warn);
  }, [isDirty, allowed]);
}

/**
 * The desktop window's close request, held while the form has unsaved input: returns whether
 * a close is waiting for an answer, and how to answer it. A browser has no such request.
 */
function useCloseRequestGuard(isDirty: boolean, allowed: { current: boolean }) {
  const [isAsking, setAsking] = useState(false);
  useEffect(() => {
    if (!isDirty || !inDesktopWindow()) return;
    let isStale = false;
    let stopListening: (() => void) | undefined;
    getCurrentWindow()
      .onCloseRequested((event) => {
        if (allowed.current) return;
        // Must be set before the handler returns, or the window closes as usual.
        event.preventDefault();
        setAsking(true);
      })
      .then((unlisten) => {
        if (isStale) unlisten();
        else stopListening = unlisten;
      })
      .catch(() => {
        // Without the window API the page still warns on in-app navigation.
      });
    return () => {
      isStale = true;
      stopListening?.();
    };
  }, [isDirty, allowed]);
  return {
    isAsking,
    keepOpen: () => setAsking(false),
    close: () => {
      allowed.current = true;
      setAsking(false);
      void getCurrentWindow().destroy();
    },
  };
}

/**
 * Asks before the page is left while the form holds unsaved input: in-app navigation to
 * another page (also from one edit URL to another), a browser reload or close, and the
 * desktop window's close. `dialog` must be rendered by the page; call `allowLeave` just
 * before leaving on purpose (after a save).
 */
export function useUnsavedGuard(isDirty: boolean) {
  const allowed = useRef(false);
  const blocker = useBlocker(
    ({ currentLocation, nextLocation }) =>
      isDirty && !allowed.current && currentLocation.pathname !== nextLocation.pathname,
  );
  useBeforeUnloadGuard(isDirty, allowed);
  const closing = useCloseRequestGuard(isDirty, allowed);
  const dialog = (
    <>
      <ConfirmDialog
        title="Leave without saving?"
        isOpen={blocker.state === "blocked"}
        onOpenChange={(isOpen) => {
          if (!isOpen && blocker.state === "blocked") blocker.reset();
        }}
        confirmLabel="Leave without saving"
        onConfirm={() => {
          if (blocker.state === "blocked") blocker.proceed();
        }}
      >
        <p>You changed this form and have not applied the changes. If you leave, they are lost.</p>
      </ConfirmDialog>
      <ConfirmDialog
        title="Discard unsaved changes and close?"
        isOpen={closing.isAsking}
        onOpenChange={(isOpen) => {
          if (!isOpen) closing.keepOpen();
        }}
        confirmLabel="Discard and close"
        onConfirm={closing.close}
      >
        <p>
          You changed this form and have not applied the changes. If you close the console now, they
          are lost.
        </p>
      </ConfirmDialog>
    </>
  );
  return {
    dialog,
    allowLeave: () => {
      allowed.current = true;
    },
  };
}
