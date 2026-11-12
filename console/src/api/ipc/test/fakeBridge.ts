/** A fake Tauri bridge: records calls, answers them from handlers, and emits events. */

import type { TauriBridge } from "../bridge";
import type { AnyRequest, AnyResponse } from "../wire";

type CommandHandler = (args: Record<string, unknown>) => unknown;
type RequestHandler = (request: AnyRequest) => AnyResponse;

/** Records Tauri calls, answers them from handlers, and lets a test emit Tauri events. */
export class FakeBridge implements TauriBridge {
  readonly calls: { command: string; args: Record<string, unknown> }[] = [];
  private readonly commands = new Map<string, CommandHandler>();
  private readonly requests = new Map<string, RequestHandler>();
  private readonly listeners = new Map<string, Set<(payload: unknown) => void>>();

  constructor() {
    this.commands.set("ipc_request", (args) => {
      const request = args.request as AnyRequest;
      const handler = this.requests.get(request.type);
      if (handler === undefined)
        throw { kind: "internal", message: `no handler for ${request.type}` };
      return handler(request);
    });
  }

  /** Answers `command` with `handler`'s result; a thrown value rejects the call. */
  command(command: string, handler: CommandHandler): this {
    this.commands.set(command, handler);
    return this;
  }

  /** Answers engine requests of `type`. */
  request(type: AnyRequest["type"], handler: RequestHandler): this {
    this.requests.set(type, handler);
    return this;
  }

  /** The engine requests sent so far, optionally of one type. */
  sent(type?: AnyRequest["type"]): AnyRequest[] {
    return this.calls
      .filter((call) => call.command === "ipc_request")
      .map((call) => call.args.request as AnyRequest)
      .filter((request) => type === undefined || request.type === type);
  }

  count(command: string): number {
    return this.calls.filter((call) => call.command === command).length;
  }

  invoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
    this.calls.push({ command, args });
    const handler = this.commands.get(command);
    if (handler === undefined)
      return Promise.reject({ kind: "internal", message: `no ${command}` });
    try {
      return Promise.resolve(handler(args) as T);
    } catch (error) {
      return Promise.reject(error);
    }
  }

  listen<T>(event: string, handler: (payload: T) => void): Promise<() => void> {
    const set = this.listeners.get(event) ?? new Set();
    const wrapped = handler as (payload: unknown) => void;
    set.add(wrapped);
    this.listeners.set(event, set);
    return Promise.resolve(() => {
      set.delete(wrapped);
    });
  }

  listenerCount(event: string): number {
    return this.listeners.get(event)?.size ?? 0;
  }

  emit(event: string, payload: unknown): void {
    for (const handler of [...(this.listeners.get(event) ?? [])]) handler(payload);
  }
}

/** Lets pending promise callbacks run. */
export async function settle(times = 5): Promise<void> {
  for (let i = 0; i < times; i++) await Promise.resolve();
}
