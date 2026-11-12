// Bridges whose `ipc_subscribe` stays unanswered until the test decides how it ends.

import { FakeBridge } from "./fakeBridge";

/** `resolveId(n)` answers the subscribe with the subscription id `n`. */
export function pendingSubscribe() {
  const bridge = new FakeBridge().command("ipc_unsubscribe", () => null);
  let resolveId: (id: number) => void = () => undefined;
  bridge.command(
    "ipc_subscribe",
    () =>
      new Promise<number>((resolve) => {
        resolveId = resolve;
      }),
  );
  return { bridge, resolveId: (id: number) => resolveId(id) };
}

/** `refuse()` answers the subscribe with the engine's refusal. */
export function refusableSubscribe() {
  let refuse: () => void = () => undefined;
  const bridge = new FakeBridge().command(
    "ipc_subscribe",
    () =>
      new Promise((_, reject) => {
        refuse = () =>
          reject({
            kind: "ipc",
            error: { code: "rejected", message: "no", field_errors: [], reason_code: null },
          });
      }),
  );
  return { bridge, refuse: () => refuse() };
}
