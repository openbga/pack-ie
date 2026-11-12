import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

/** Idle time before content is hidden, in minutes (compliance P11: a workstation left open). */
export const DEFAULT_IDLE_MINUTES = 15;
export const MIN_IDLE_MINUTES = 1;
export const MAX_IDLE_MINUTES = 240;

const STORAGE_KEY = "packie.idleMinutes";
/** How often the clock is checked; a minute of idle time is never off by more than this. */
const CHECK_MS = 1000;
const ACTIVITY_EVENTS = ["pointerdown", "pointermove", "keydown", "wheel", "focusin"] as const;

interface Idle {
  /** True from the moment the idle time passes, with no activity, until `resume`. */
  idle: boolean;
  /** Dismisses the "Continue" cover and restarts the idle clock. */
  resume: () => void;
  minutes: number;
  setMinutes: (minutes: number) => void;
  /** Calls `listener` each time the console goes idle, once per idle period; returns the unsubscribe. */
  subscribe: (listener: () => void) => () => void;
}

const IdleContext = createContext<Idle>({
  idle: false,
  resume: () => {},
  minutes: DEFAULT_IDLE_MINUTES,
  setMinutes: () => {},
  subscribe: () => () => {},
});

/** A whole number of minutes in range, or the default for anything else. */
function validIdleMinutes(value: unknown): number {
  const minutes = typeof value === "string" ? Number(value) : value;
  return typeof minutes === "number" &&
    Number.isInteger(minutes) &&
    minutes >= MIN_IDLE_MINUTES &&
    minutes <= MAX_IDLE_MINUTES
    ? minutes
    : DEFAULT_IDLE_MINUTES;
}

function storedMinutes(): number {
  try {
    return validIdleMinutes(localStorage.getItem(STORAGE_KEY));
  } catch {
    return DEFAULT_IDLE_MINUTES;
  }
}

/**
 * Tracks when the operator last did something in this window. After `minutes` without
 * activity the console goes idle: whatever holds message content lets go of it (`useOnIdle`)
 * and the whole console is covered until the operator chooses Continue (no login: the engine's
 * access control is the OS account). The setting is the console's own, kept on this computer.
 */
export function IdleProvider({ children }: { children: ReactNode }) {
  const [minutes, setMinutesState] = useState(storedMinutes);
  // Refs: pointer movement must not re-render the app, and the clock reads the idle flag
  // between renders.
  const lastActive = useRef(Date.now());
  const idleNow = useRef(false);
  const listeners = useRef(new Set<() => void>());
  const [idle, setIdle] = useState(false);

  const touch = useCallback(() => {
    lastActive.current = Date.now();
  }, []);
  useEffect(() => {
    // Synchronizes with browser events, which live outside React.
    for (const type of ACTIVITY_EVENTS) document.addEventListener(type, touch, { passive: true });
    return () => {
      for (const type of ACTIVITY_EVENTS) document.removeEventListener(type, touch);
    };
  }, [touch]);

  useEffect(() => {
    // Synchronizes with the clock, which lives outside React.
    const timer = setInterval(() => {
      if (idleNow.current || Date.now() - lastActive.current < minutes * 60_000) return;
      idleNow.current = true;
      setIdle(true);
      // Synchronously, so a read that settles a moment later already finds its content dropped.
      for (const listener of [...listeners.current]) {
        try {
          listener();
        } catch (error) {
          reportError(error);
        }
      }
    }, CHECK_MS);
    return () => clearInterval(timer);
  }, [minutes]);

  const resume = useCallback(() => {
    idleNow.current = false;
    lastActive.current = Date.now();
    setIdle(false);
  }, []);
  const subscribe = useCallback((listener: () => void) => {
    listeners.current.add(listener);
    return () => {
      listeners.current.delete(listener);
    };
  }, []);
  const setMinutes = useCallback((next: number) => {
    const valid = validIdleMinutes(next);
    setMinutesState(valid);
    try {
      localStorage.setItem(STORAGE_KEY, String(valid));
    } catch {
      // The setting lasts until the window closes.
    }
  }, []);

  const value = useMemo(
    () => ({ idle, resume, minutes, setMinutes, subscribe }),
    [idle, resume, minutes, setMinutes, subscribe],
  );
  return <IdleContext value={value}>{children}</IdleContext>;
}

export function useIdle(): Idle {
  return useContext(IdleContext);
}

/**
 * Runs `onIdle` once each time the console goes idle. Whatever holds message content or audit
 * details uses it to let go of them: the screens cannot rely on the cover alone, since content
 * under it is still in memory and in the page.
 */
export function useOnIdle(onIdle: () => void): void {
  const { subscribe } = useIdle();
  const latest = useRef(onIdle);
  useEffect(() => {
    latest.current = onIdle;
  });
  useEffect(() => subscribe(() => latest.current()), [subscribe]);
}
