// What the opt-in live suites (`live.test.tsx`, `liveMessages.test.tsx`) share: the socket to a
// RUNNING engine, a bridge that plays the part of the Tauri shell over it (src-tauri), polling,
// and the channel the bridge suites act on. Nothing here runs without PACKIE_LIVE_IPC.

import type { ClientFrame, Request, Response, ServerFrame } from "@/ipc/generated";
import type { ConnectionPayload, TauriBridge } from "../bridge";
import { IpcEngineApi } from "../IpcEngineApi";

declare const process: { env: Record<string, string | undefined> };

interface NodeSocket {
  on(event: "data", listener: (chunk: Uint8Array) => void): void;
  on(event: "error" | "connect" | "close", listener: (error?: unknown) => void): void;
  write(data: Uint8Array | string): void;
  destroy(): void;
}
interface NodeNet {
  createConnection(target: string | { host: string; port: number }): NodeSocket;
}

/** An environment variable of the run (the vitest types here carry no `process`). */
export function liveEnv(name: string): string | undefined {
  return process.env[name];
}

/** The engine's socket path; the suites are skipped without it. */
export const SOCKET = liveEnv("PACKIE_LIVE_IPC");

export async function loadNet(): Promise<NodeNet> {
  const moduleName = "node:net";
  return (await import(/* @vite-ignore */ moduleName)) as unknown as NodeNet;
}

/** The Rust shell's job for one connection: frames out, answers and events back. */
async function socketBridge(
  path: string,
  client: string,
): Promise<{ bridge: TauriBridge; close: () => void }> {
  const net = await loadNet();
  const socket = net.createConnection(path);
  await new Promise((resolve, reject) => {
    socket.on("connect", resolve);
    socket.on("error", reject);
  });
  let buffer = new Uint8Array(0);
  let nextId = 1;
  const pending = new Map<number, (frame: ServerFrame) => void>();
  const listeners = new Map<string, Set<(payload: unknown) => void>>();
  const emit = (name: string, payload: unknown) => {
    for (const listener of listeners.get(name) ?? []) listener(payload);
  };
  socket.on("data", (chunk) => {
    const joined = new Uint8Array(buffer.length + chunk.length);
    joined.set(buffer);
    joined.set(chunk, buffer.length);
    buffer = joined;
    while (buffer.length >= 4) {
      const length = new DataView(buffer.buffer, buffer.byteOffset).getUint32(0);
      if (buffer.length < 4 + length) return;
      const frame = JSON.parse(
        new TextDecoder().decode(buffer.slice(4, 4 + length)),
      ) as ServerFrame;
      buffer = buffer.slice(4 + length);
      if (frame.type === "event")
        emit("packie://event", { subscription: frame.subscription, event: frame.event });
      else pending.get(frame.id)?.(frame);
    }
  });
  const send = (request: Request) =>
    new Promise<Response>((resolve, reject) => {
      const id = nextId++;
      pending.set(id, (frame) => {
        pending.delete(id);
        if (frame.type === "response") resolve(frame.result);
        else if (frame.type === "error") reject({ kind: "ipc", error: frame.error });
      });
      const body = new TextEncoder().encode(JSON.stringify({ id, request } satisfies ClientFrame));
      const framed = new Uint8Array(4 + body.length);
      new DataView(framed.buffer).setUint32(0, body.length);
      framed.set(body, 4);
      socket.write(framed);
    });
  const hello = await send({ type: "hello", client });
  const engine = hello.type === "hello" ? hello : null;
  const connection: ConnectionPayload = {
    state: "connected",
    endpoint: path,
    error: null,
    attempt: 0,
    engine: engine && {
      engine_version: engine.engine_version,
      protocol_version: engine.protocol_version,
      node_id: engine.node_id,
      node_name: engine.node_name,
    },
    since: new Date().toISOString(),
  };
  const bridge: TauriBridge = {
    async invoke<T>(command: string, args?: Record<string, unknown>) {
      switch (command) {
        case "ipc_request":
          return (await send(args?.request as Request)) as T;
        case "ipc_subscribe": {
          const response = await send({ type: "subscribe", topic: args?.topic } as Request);
          return (response.type === "subscribed" ? response.subscription : 0) as T;
        }
        case "ipc_unsubscribe":
          await send({ type: "unsubscribe", subscription: args?.subscription as number });
          return null as T;
        case "ipc_connection":
          return connection as T;
        case "read_bootstrap":
          return null as T;
        case "app_version":
          return "live-test" as T;
        default:
          throw { kind: "internal", message: `${command} is not part of the live harness` };
      }
    },
    async listen<T>(event: string, handler: (payload: T) => void) {
      const set = listeners.get(event) ?? new Set();
      set.add(handler as (payload: unknown) => void);
      listeners.set(event, set);
      return () => set.delete(handler as (payload: unknown) => void);
    },
  };
  return { bridge, close: () => socket.destroy() };
}

/** Polls `read` until `done` accepts it; fails with what was last seen. */
export async function until<T>(
  what: string,
  read: () => Promise<T>,
  done: (value: T) => boolean,
  timeoutMs = 20_000,
): Promise<T> {
  const deadline = Date.now() + timeoutMs;
  let last: T | undefined;
  while (Date.now() < deadline) {
    last = await read();
    if (done(last)) return last;
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`timed out waiting for ${what}; last seen: ${JSON.stringify(last)}`);
}

/** A connection to the engine behind `SOCKET`, as the production API; `close` ends both. */
export async function connect(client = "packie-console live test") {
  const { bridge, close } = await socketBridge(SOCKET ?? "", client);
  const api = new IpcEngineApi(bridge);
  return {
    api,
    close: () => {
      api.dispose();
      close();
    },
  };
}

export const BRIDGE = "bridge-1";
export const BRIDGE_LABEL = "Instrument to system";

/**
 * The channel the generic tests act on: the bridge when the engine has one, else its first
 * running channel. Resolved once, before the tests.
 */
export const target = { id: BRIDGE, label: BRIDGE_LABEL, hasBridge: true };

/** The target channel of a fresh snapshot. */
export async function bridgeChannel(api: IpcEngineApi) {
  const channel = (await api.getSnapshot()).channels.find((c) => c.id === target.id);
  if (channel === undefined) throw new Error(`the engine has no channel ${target.id}`);
  return channel;
}

/** Leaves the target channel as it was found, running, whatever a test did to it. */
export async function leaveRunning(api: IpcEngineApi) {
  const channel = await bridgeChannel(api);
  if (channel.state === "stopped" && channel.allowedActions?.includes("resume")) {
    await api.resumeChannel(target.id);
  }
  await until(
    `${target.id} running`,
    () => bridgeChannel(api),
    (c) => c.state === "running" || c.state === "degraded" || c.state === "retrying",
  );
}
