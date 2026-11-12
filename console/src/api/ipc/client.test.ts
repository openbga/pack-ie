import { describe, expect, it } from "vitest";
import type { Event } from "@/ipc/generated";
import type { ConnectionPayload } from "./bridge";
import { IpcClient } from "./client";
import { FakeBridge, settle } from "./test/fakeBridge";
import { pendingSubscribe, refusableSubscribe } from "./test/pendingSubscribe";

const lagged = (missed: number): Event => ({ type: "lagged", missed });

function connection(
  state: ConnectionPayload["state"],
  extra: Partial<ConnectionPayload> = {},
): ConnectionPayload {
  return {
    state,
    endpoint: "/run/packie.sock",
    error: null,
    attempt: 0,
    engine: null,
    since: "2026-11-06T12:00:00Z",
    ...extra,
  };
}

describe("request", () => {
  it("sends the request and returns the answer", async () => {
    const bridge = new FakeBridge().request("get_snapshot", () => ({ type: "done" }));
    const client = new IpcClient(bridge);
    await expect(client.request({ type: "get_snapshot" }, "done")).resolves.toEqual({
      type: "done",
    });
    expect(bridge.calls[0]).toEqual({
      command: "ipc_request",
      args: { request: { type: "get_snapshot" } },
    });
  });

  it("rejects with an engine error when the command fails", async () => {
    const bridge = new FakeBridge().request("get_snapshot", () => {
      throw {
        kind: "ipc",
        error: { code: "unavailable", message: "starting", field_errors: [], reason_code: null },
      };
    });
    await expect(
      new IpcClient(bridge).request({ type: "get_snapshot" }, "snapshot"),
    ).rejects.toEqual({
      code: "unavailable",
      message: "starting",
      fieldErrors: [],
      correlationId: "",
    });
  });

  it("rejects when the engine answers with another kind of response", async () => {
    const bridge = new FakeBridge().request("get_snapshot", () => ({ type: "done" }));
    await expect(
      new IpcClient(bridge).request({ type: "get_snapshot" }, "snapshot"),
    ).rejects.toMatchObject({
      code: "internal",
    });
  });
});

describe("subscriptions", () => {
  function setup() {
    let next = 1;
    const bridge = new FakeBridge()
      .command("ipc_subscribe", () => next++)
      .command("ipc_unsubscribe", () => null);
    return { bridge, client: new IpcClient(bridge) };
  }

  it("routes events by subscription id", async () => {
    const { bridge, client } = setup();
    const a: Event[] = [];
    const b: Event[] = [];
    client.subscribe({ type: "alerts" }, (e) => a.push(e));
    client.subscribe({ type: "snapshots" }, (e) => b.push(e));
    await settle();
    bridge.emit("packie://event", { subscription: 1, event: lagged(1) });
    bridge.emit("packie://event", { subscription: 2, event: lagged(2) });
    expect(a).toEqual([lagged(1)]);
    expect(b).toEqual([lagged(2)]);
    expect(bridge.count("ipc_subscribe")).toBe(2);
    expect(bridge.calls.find((c) => c.command === "ipc_subscribe")?.args).toEqual({
      topic: { type: "alerts" },
    });
    expect(bridge.listenerCount("packie://event")).toBe(1);
  });

  it("keeps events that arrive before the id is known and delivers them in order", async () => {
    const { bridge, resolveId } = pendingSubscribe();
    const client = new IpcClient(bridge);
    const seen: Event[] = [];
    client.subscribe({ type: "snapshots" }, (e) => seen.push(e));
    await settle();
    bridge.emit("packie://event", { subscription: 7, event: lagged(1) });
    bridge.emit("packie://event", { subscription: 7, event: lagged(2) });
    expect(seen).toEqual([]);
    resolveId(7);
    await settle(10);
    expect(seen).toEqual([lagged(1), lagged(2)]);
    bridge.emit("packie://event", { subscription: 7, event: lagged(3) });
    expect(seen).toHaveLength(3);
  });

  it("unsubscribing stops delivery and tells the console side once", async () => {
    const { bridge, client } = setup();
    const seen: Event[] = [];
    const stop = client.subscribe({ type: "snapshots" }, (e) => seen.push(e));
    await settle();
    stop();
    stop();
    bridge.emit("packie://event", { subscription: 1, event: lagged(1) });
    expect(seen).toEqual([]);
    const unsubscribes = bridge.calls.filter((c) => c.command === "ipc_unsubscribe");
    expect(unsubscribes).toEqual([{ command: "ipc_unsubscribe", args: { subscription: 1 } }]);
  });

  it("unsubscribing before the id is known releases it as soon as it arrives", async () => {
    const { bridge, resolveId } = pendingSubscribe();
    const client = new IpcClient(bridge);
    const seen: Event[] = [];
    const stop = client.subscribe({ type: "snapshots" }, (e) => seen.push(e));
    await settle();
    stop();
    resolveId(4);
    await settle(10);
    expect(bridge.calls.filter((c) => c.command === "ipc_unsubscribe")).toEqual([
      { command: "ipc_unsubscribe", args: { subscription: 4 } },
    ]);
    bridge.emit("packie://event", { subscription: 4, event: lagged(1) });
    expect(seen).toEqual([]);
  });

  it("subscribes once the engine connects when it was away at first", async () => {
    let up = false;
    const bridge = new FakeBridge()
      .command("ipc_subscribe", () => {
        if (!up) throw { kind: "disconnected", message: "no engine" };
        return 3;
      })
      .command("ipc_unsubscribe", () => null);
    const client = new IpcClient(bridge);
    const seen: Event[] = [];
    client.subscribe({ type: "snapshots" }, (e) => seen.push(e));
    await settle(10);
    expect(bridge.count("ipc_subscribe")).toBe(1);

    bridge.emit("packie://connection", connection("reconnecting"));
    expect(bridge.count("ipc_subscribe")).toBe(1);
    up = true;
    bridge.emit("packie://connection", connection("connected"));
    await settle(10);
    expect(bridge.count("ipc_subscribe")).toBe(2);
    bridge.emit("packie://event", { subscription: 3, event: lagged(1) });
    expect(seen).toEqual([lagged(1)]);
    // Later reconnects do not subscribe again: the Rust side resubscribes under the same id.
    bridge.emit("packie://connection", connection("connected"));
    await settle(10);
    expect(bridge.count("ipc_subscribe")).toBe(2);
  });

  it("reports a subscription the engine refuses and does not ask again", async () => {
    const refusal = {
      kind: "ipc",
      error: {
        code: "rejected",
        message: "Transcript in use by another client.",
        field_errors: [],
        reason_code: "transcript_in_use",
      },
    };
    const bridge = new FakeBridge().command("ipc_subscribe", () => {
      throw refusal;
    });
    const client = new IpcClient(bridge);
    const errors: unknown[] = [];
    client.subscribe(
      { type: "traffic", channel_id: "bridge-1" },
      () => undefined,
      (error) => errors.push(error),
    );
    await settle(10);
    expect(errors).toEqual([
      {
        code: "conflict",
        message: "Transcript in use by another client.",
        fieldErrors: [],
        rejection: "transcript_in_use",
        correlationId: "",
      },
    ]);
    bridge.emit("packie://connection", connection("connected"));
    await settle(10);
    expect(bridge.count("ipc_subscribe")).toBe(1);
  });

  it("does not report a refusal to a caller that gave up", async () => {
    const { bridge, refuse } = refusableSubscribe();
    const errors: unknown[] = [];
    const stop = new IpcClient(bridge).subscribe(
      { type: "snapshots" },
      () => undefined,
      (error) => errors.push(error),
    );
    await settle(10);
    stop();
    refuse();
    await settle(10);
    expect(errors).toEqual([]);
  });

  it("does not subscribe after the caller gave up while the engine was away", async () => {
    const bridge = new FakeBridge().command("ipc_subscribe", () => {
      throw { kind: "disconnected", message: "no engine" };
    });
    const client = new IpcClient(bridge);
    const stop = client.subscribe({ type: "snapshots" }, () => undefined);
    await settle(10);
    stop();
    bridge.emit("packie://connection", connection("connected"));
    await settle(10);
    expect(bridge.count("ipc_subscribe")).toBe(1);
  });
});

describe("connection", () => {
  it("delivers the current state first, then every change", async () => {
    const bridge = new FakeBridge().command("ipc_connection", () => connection("connected"));
    const client = new IpcClient(bridge);
    const states: string[] = [];
    const stop = client.watchConnection((p) => states.push(p.state));
    await settle(10);
    expect(states).toEqual(["connected"]);
    bridge.emit("packie://connection", connection("disconnected", { error: "gone" }));
    bridge.emit("packie://connection", connection("reconnecting"));
    expect(states).toEqual(["connected", "disconnected", "reconnecting"]);
    expect(client.lastConnection?.state).toBe("reconnecting");
    stop();
    bridge.emit("packie://connection", connection("connected"));
    expect(states).toHaveLength(3);
  });

  it("prefers an event that overtook the initial answer", async () => {
    let answer: (payload: ConnectionPayload) => void = () => undefined;
    const bridge = new FakeBridge().command(
      "ipc_connection",
      () =>
        new Promise<ConnectionPayload>((resolve) => {
          answer = resolve;
        }),
    );
    const client = new IpcClient(bridge);
    const states: string[] = [];
    client.watchConnection((p) => states.push(p.state));
    await settle(10);
    bridge.emit("packie://connection", connection("connected"));
    answer(connection("disconnected"));
    await settle(10);
    expect(states).toEqual(["connected"]);
  });

  it("stops listening when disposed", async () => {
    const bridge = new FakeBridge().command("ipc_connection", () => connection("connected"));
    const client = new IpcClient(bridge);
    client.watchConnection(() => undefined);
    await settle(10);
    expect(bridge.listenerCount("packie://connection")).toBe(1);
    client.dispose();
    expect(bridge.listenerCount("packie://connection")).toBe(0);
  });
});
