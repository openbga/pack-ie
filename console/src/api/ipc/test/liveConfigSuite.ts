// The configuration tests of `live.test.tsx` (its `describe` registers them): a label edit that
// is put back, the engine's validation at the console's field paths, and a stale document.

import { expect, it } from "vitest";
import type { IpcEngineApi } from "../IpcEngineApi";
import { BRIDGE, bridgeChannel, connect, leaveRunning, target, until } from "./liveHarness";

const REFUSAL_ALERTS = ["config_rejected", "config_apply_failed"];

/**
 * A refused document raises an alert that only the next successful apply clears; applying the
 * active document as it is clears it (and adds one version), so a test that provoked a refusal
 * leaves no alert behind.
 */
async function clearRefusalAlerts(api: IpcEngineApi) {
  const open = await api.listAlerts("open");
  if (open.some((alert) => REFUSAL_ALERTS.includes(alert.kind))) {
    await api.applyConfig(
      await api.getConfig(),
      "live test: clear the alert of a refused document",
    );
  }
}

type ConfigDocument = Awaited<ReturnType<IpcEngineApi["getConfig"]>>;

/**
 * Documents with one kind of problem each. The engine reports the first malformed value of a
 * document before it checks the rest, so each problem is asked about in a document of its own.
 */
function validationCases(
  doc: ConfigDocument,
): [string, ConfigDocument, { path: string; message: string | RegExp }][] {
  const listener = doc.endpoints.findIndex((e) => e.kind === "tcp_listener");
  if (listener < 0) throw new Error("the engine's document has no TCP listener to test with");
  // A bridge from an endpoint to itself needs a bridge: use the engine's, or add one.
  const bridgeId = target.hasBridge ? BRIDGE : "live-same-endpoint";
  const bridgeIndex = target.hasBridge
    ? doc.channels.findIndex((c) => c.id === BRIDGE)
    : doc.channels.length;
  const serial = doc.endpoints.length;
  // The engine reports the first malformed value of a document before it checks the rest,
  // so each kind of problem is asked about in a document of its own.
  const badCidr: typeof doc = {
    ...doc,
    endpoints: doc.endpoints.map((e, i) =>
      i === listener && e.kind === "tcp_listener" ? { ...e, allowlist: ["10.0.0.0/33"] } : e,
    ),
  };
  const badBaud: typeof doc = {
    ...doc,
    endpoints: [
      ...doc.endpoints,
      {
        kind: "serial",
        id: "live-serial",
        label: "Live serial",
        enabled: true,
        port: "COM9",
        baudRate: 0,
        dataBits: 8,
        parity: "none",
        stopBits: 1,
        flowControl: "none",
      },
    ],
  };
  const sameEndpoint: typeof doc = target.hasBridge
    ? {
        ...doc,
        channels: doc.channels.map((c) =>
          c.id === bridgeId && c.kind === "bridge" ? { ...c, endpointBId: c.endpointAId } : c,
        ),
      }
    : {
        ...doc,
        channels: [
          ...doc.channels,
          {
            kind: "bridge",
            id: bridgeId,
            label: "Live bridge to itself",
            enabled: true,
            endpointAId: doc.endpoints[0]?.id ?? "",
            endpointBId: doc.endpoints[0]?.id ?? "",
            pairing: "parallel",
            pairingReceiver: null,
            bufferBytes: 65_536,
            retry: { initialMs: 1000, maxMs: 30_000, maxAttempts: null },
            tapFramer: "none",
            tapRetention: { days: 30, maxMessages: null },
          },
        ],
      };
  const cases: [string, typeof doc, { path: string; message: string | RegExp }][] = [
    [
      "a CIDR block that is not one",
      badCidr,
      {
        path: `endpoints[${listener}].allowlist[0]`,
        message: /^not an IP address or CIDR block/,
      },
    ],
    [
      "a baud rate of 0",
      badBaud,
      { path: `endpoints[${serial}].baudRate`, message: "must be greater than 0" },
    ],
    [
      "a bridge from an endpoint to itself",
      sameEndpoint,
      { path: `channels[${bridgeIndex}].endpointBId`, message: "must differ from endpoint_a" },
    ],
  ];
  return cases;
}

export function configTests() {
  it("applies a label edit with its note, lists the version, and restores the label", async () => {
    const { api, close } = await connect();
    const before = await api.getConfig();
    const original = before.channels.find((c) => c.id === target.id);
    if (original === undefined) throw new Error("the channel is not in the document");
    const relabel = (document: typeof before, label: string) => ({
      ...document,
      channels: document.channels.map((c) => (c.id === target.id ? { ...c, label } : c)),
    });
    try {
      const edited = relabel(before, `${original.label} (live test)`);
      expect((await api.validateConfig(edited)).issues).toEqual([]);
      const applied = await api.applyConfig(edited, "live test: relabel the channel");
      expect(applied.version).toBe(before.version + 1);
      expect(applied.failures).toEqual([]);

      const after = await api.getConfig();
      expect(after.version).toBe(applied.version);
      expect(after.channels.find((c) => c.id === target.id)?.label).toBe(
        `${original.label} (live test)`,
      );
      const [newest] = await api.listConfigVersions();
      expect(newest).toMatchObject({ version: applied.version, current: true });
      expect(newest?.note).toBe("live test: relabel the channel");
      await until(
        "the new label in the snapshot",
        () => bridgeChannel(api),
        (c) => c.label === `${original.label} (live test)` && c.state !== "stopped",
      );
      expect(
        (await api.getConfigVersion(before.version)).channels.find((c) => c.id === target.id)
          ?.label,
      ).toBe(original.label);
    } finally {
      const now = await api.getConfig();
      if (now.channels.find((c) => c.id === target.id)?.label !== original.label) {
        await api.applyConfig(relabel(now, original.label), "live test: restore the label");
      }
      await leaveRunning(api);
      const restored = await api.getConfig();
      expect(restored.channels.find((c) => c.id === target.id)).toEqual(original);
      close();
    }
  }, 90_000);

  it("reports real validation problems at the console's field paths, and refuses to apply them", async () => {
    const { api, close } = await connect();
    let rejectedId = "";
    try {
      const doc = await api.getConfig();
      const cases = validationCases(doc);
      for (const [name, document, expected] of cases) {
        const result = await api.validateConfig(document);
        expect(result.ok, name).toBe(false);
        // The engine names the rule that was broken (`code`) next to the field and the words.
        // (A bridge added to an engine that has none shares its endpoint with other channels, so
        // the engine may add a finding about that: the one asked about must be among them.)
        expect(result.issues, name).toContainEqual({
          severity: "error",
          path: expected.path,
          message:
            expected.message instanceof RegExp
              ? expect.stringMatching(expected.message)
              : expected.message,
          code: expect.any(String),
        });
        // Applying the same document is refused with the same field, and changes nothing.
        await expect(
          api.applyConfig(document, "live test: must be refused"),
          name,
        ).rejects.toMatchObject({
          code: "validation",
          fieldErrors: expect.arrayContaining([expect.objectContaining({ field: expected.path })]),
        });
      }
      expect((await api.getConfig()).version).toBe(doc.version);

      // The refusals are alerts of the engine, open until a document is applied.
      const open = await until(
        "the alert about the refused document",
        () => api.listAlerts("open"),
        (list) => list.some((alert) => alert.kind === "config_rejected"),
        10_000,
      );
      rejectedId = open.find((alert) => alert.kind === "config_rejected")?.id ?? "";
      expect(open.find((alert) => alert.kind === "config_rejected")).toMatchObject({
        state: "open",
        clearedAt: null,
        occurrences: expect.any(Number),
        title: expect.any(String),
        nextAction: expect.any(String),
      });
    } finally {
      await clearRefusalAlerts(api);
      close();
    }
    // Cleared, it stays in the history the engine keeps.
    const again = await connect();
    try {
      const cleared = await until(
        "the cleared alert in the history",
        () => again.api.listAlerts("cleared"),
        (list) => list.some((alert) => alert.id === rejectedId),
        10_000,
      );
      expect(cleared.find((alert) => alert.id === rejectedId)).toMatchObject({
        kind: "config_rejected",
        state: "cleared",
        clearedAt: expect.any(String),
      });
      expect(
        (await again.api.listAlerts("open")).filter((a) => REFUSAL_ALERTS.includes(a.kind)),
      ).toEqual([]);
    } finally {
      again.close();
    }
  });

  it("refuses a document edited from an older version", async () => {
    const { api, close } = await connect();
    try {
      const doc = await api.getConfig();
      await expect(
        api.applyConfig({ ...doc, version: doc.version - 1 }, "live test: stale"),
      ).rejects.toMatchObject({ code: "conflict" });
      expect((await api.getConfig()).version).toBe(doc.version);
    } finally {
      await clearRefusalAlerts(api);
      close();
    }
  });
}
