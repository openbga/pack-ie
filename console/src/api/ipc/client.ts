import type { Event, Topic } from "@/ipc/generated";
import type { Unsubscribe } from "../engine";
import type { EngineError } from "../types";
import {
  CONNECTION_CHANNEL,
  type ConnectionPayload,
  EVENT_CHANNEL,
  type EventPayload,
  type TauriBridge,
} from "./bridge";
import { engineError, fromCommandError } from "./errors";
import { isRecord } from "./util";
import type { AnyRequest, AnyResponse, ResponseOf, WireConfig } from "./wire";

/** Events that arrive before their subscription id is known are kept, up to these bounds. */
const EARLY_EVENTS_PER_SUBSCRIPTION = 256;
const EARLY_SUBSCRIPTIONS = 32;

/** Whether a rejected `ipc_subscribe` is the engine saying no (as opposed to not answering). */
function isRefusal(error: unknown): boolean {
  return isRecord(error) && error.kind === "ipc";
}

/**
 * Requests and subscriptions over the Tauri commands, with their failure modes handled: every
 * rejection is an `EngineError`, events are routed by subscription id (and kept when they win
 * the race against the id), and a subscription made while the engine is away starts when the
 * connection comes up. Console-side subscription ids survive engine reconnects (the Rust side
 * resubscribes), so a stream continues under the same handler.
 */
export class IpcClient {
  private readonly handlers = new Map<number, (event: Event) => void>();
  private readonly early = new Map<number, Event[]>();
  private readonly connectionHandlers = new Set<(payload: ConnectionPayload) => void>();
  private readonly unlisteners: (() => void)[] = [];
  private eventListener: Promise<void> | null = null;
  private connectionListener: Promise<void> | null = null;
  private connection: ConnectionPayload | null = null;

  constructor(readonly bridge: TauriBridge) {}

  /**
   * Sends `request` and returns the answer, which must be of type `expected`. `document` is the
   * configuration the request carries, if any: it refines the field paths of a refusal.
   */
  async request<T extends AnyResponse["type"]>(
    request: AnyRequest,
    expected: T,
    document?: WireConfig,
  ): Promise<ResponseOf<T>> {
    let response: AnyResponse;
    try {
      response = await this.bridge.invoke<AnyResponse>("ipc_request", { request });
    } catch (error) {
      throw fromCommandError(error, document);
    }
    if (response.type !== expected) {
      throw engineError(
        "internal",
        `The engine answered ${response.type} where ${expected} was expected.`,
      );
    }
    return response as ResponseOf<T>;
  }

  // ── Subscriptions ────────────────────────────────────────────────────────────────────

  /**
   * Streams `topic`. An engine that refuses the subscription (`rejected`, such as a transcript
   * another client holds) is reported to `onError` and not asked again; one that cannot be
   * reached is asked again once the connection is up.
   */
  subscribe(
    topic: Topic,
    onEvent: (event: Event) => void,
    onError?: (error: EngineError) => void,
  ): Unsubscribe {
    let cancelled = false;
    let id: number | null = null;

    const open = async (): Promise<void> => {
      await this.listenForEvents();
      if (cancelled) return;
      const subscription = await this.bridge.invoke<number>("ipc_subscribe", { topic });
      if (cancelled) {
        this.release(subscription);
        return;
      }
      id = subscription;
      this.handlers.set(subscription, onEvent);
      for (const event of this.early.get(subscription) ?? []) onEvent(event);
      this.early.delete(subscription);
    };

    const start = (): Promise<void> =>
      open().catch((error: unknown) => {
        if (!isRefusal(error)) {
          // No engine to subscribe to yet: try again once the connection is up.
          this.whenConnected(() => !cancelled && void start());
        } else if (!cancelled) onError?.(fromCommandError(error));
      });
    void start();

    return () => {
      if (cancelled) return;
      cancelled = true;
      if (id !== null) {
        this.handlers.delete(id);
        this.release(id);
      }
    };
  }

  private release(subscription: number): void {
    this.early.delete(subscription);
    this.bridge.invoke("ipc_unsubscribe", { subscription }).catch(() => undefined);
  }

  private listenForEvents(): Promise<void> {
    this.eventListener ??= this.bridge
      .listen<EventPayload>(EVENT_CHANNEL, (payload) => this.route(payload))
      .then((unlisten) => {
        this.unlisteners.push(unlisten);
      })
      .catch((error: unknown) => {
        this.eventListener = null;
        throw error;
      });
    return this.eventListener;
  }

  private route({ subscription, event }: EventPayload): void {
    const handler = this.handlers.get(subscription);
    if (handler !== undefined) {
      handler(event);
      return;
    }
    const kept = this.early.get(subscription) ?? [];
    if (kept.length < EARLY_EVENTS_PER_SUBSCRIPTION) kept.push(event);
    this.early.set(subscription, kept);
    const oldest = this.early.keys().next();
    if (this.early.size > EARLY_SUBSCRIPTIONS && !oldest.done) this.early.delete(oldest.value);
  }

  // ── Connection ───────────────────────────────────────────────────────────────────────

  /** The last connection state seen, if any. */
  get lastConnection(): ConnectionPayload | null {
    return this.connection;
  }

  /** Asks the console's Rust side where the engine connection stands now. */
  async currentConnection(): Promise<ConnectionPayload> {
    try {
      const payload = await this.bridge.invoke<ConnectionPayload>("ipc_connection");
      this.connection = payload;
      return payload;
    } catch (error) {
      throw fromCommandError(error);
    }
  }

  /** The current state first, then every change. */
  watchConnection(onChange: (payload: ConnectionPayload) => void): Unsubscribe {
    let live = true;
    let seen = false;
    const handler = (payload: ConnectionPayload) => {
      seen = true;
      onChange(payload);
    };
    this.connectionHandlers.add(handler);
    this.listenForConnection()
      .then(() => this.currentConnection())
      .then((payload) => {
        if (live && !seen) onChange(payload);
      })
      .catch(() => undefined);
    return () => {
      live = false;
      this.connectionHandlers.delete(handler);
    };
  }

  private listenForConnection(): Promise<void> {
    this.connectionListener ??= this.bridge
      .listen<ConnectionPayload>(CONNECTION_CHANNEL, (payload) => {
        this.connection = payload;
        for (const handler of [...this.connectionHandlers]) handler(payload);
      })
      .then((unlisten) => {
        this.unlisteners.push(unlisten);
      })
      .catch((error: unknown) => {
        this.connectionListener = null;
        throw error;
      });
    return this.connectionListener;
  }

  /** Runs `action` once, when the connection next reports `connected`. */
  private whenConnected(action: () => void): void {
    const handler = (payload: ConnectionPayload) => {
      if (payload.state !== "connected") return;
      this.connectionHandlers.delete(handler);
      action();
    };
    this.connectionHandlers.add(handler);
    this.listenForConnection().catch(() => this.connectionHandlers.delete(handler));
  }

  /** Stops listening to Tauri events; subscriptions end silently. */
  dispose(): void {
    for (const unlisten of this.unlisteners.splice(0)) unlisten();
    this.handlers.clear();
    this.early.clear();
    this.connectionHandlers.clear();
    this.eventListener = null;
    this.connectionListener = null;
  }
}
