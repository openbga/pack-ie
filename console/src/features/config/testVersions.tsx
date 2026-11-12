// Version history test data: three configuration versions, the engine's plan between them
// and a stub engine that serves them.
import { vi } from "vitest";
import type { ApplyPlan, ConfigDocument, ConfigVersionSummary } from "@/api/types";
import { sampleConfig, stubApi } from "./testFixtures";
import { VersionHistory } from "./VersionHistory";

export const versionRoutes = [{ path: "/settings/versions", element: <VersionHistory /> }];

const VERSIONS: ConfigVersionSummary[] = [
  {
    version: 3,
    appliedAt: "2026-11-05T09:00:00Z",
    appliedBy: "LAB\\jdoe",
    note: "Edited endpoint LIS host",
    endpoints: 3,
    channels: 2,
    verifications: [],
    outcomes: [
      { channelId: "chem-bridge", outcome: "restarted" },
      { channelId: "results", outcome: "failed", reason: "Cannot bind COM3: access denied" },
    ],
    current: true,
  },
  {
    version: 2,
    appliedAt: "2026-11-04T09:00:00Z",
    appliedBy: "LAB\\mlopez",
    note: "Added channel Results to LIS",
    endpoints: 3,
    channels: 1,
    verifications: [
      { by: "LAB\\mlopez", at: "2026-11-04T15:00:00Z", note: "VR-2026-018" },
      { by: "LAB\\jdoe", at: "2026-11-05T08:00:00Z", note: "VR-2026-019" },
    ],
    outcomes: [
      { channelId: "chem-bridge", outcome: "unchanged" },
      { channelId: "results", outcome: "started" },
    ],
    current: false,
  },
  {
    version: 1,
    appliedAt: "2026-11-03T09:00:00Z",
    appliedBy: null,
    note: null,
    endpoints: 1,
    channels: 0,
    verifications: [],
    outcomes: [],
    current: false,
  },
];

export function documents(): Record<number, ConfigDocument> {
  const current = { ...sampleConfig(), version: 3 };
  const second: ConfigDocument = {
    ...current,
    version: 2,
    channels: current.channels.filter((channel) => channel.id !== "results"),
    alertThresholds: { ...current.alertThresholds, queueDepthWarning: 100 },
  };
  const first: ConfigDocument = {
    ...second,
    version: 1,
    endpoints: second.endpoints.slice(0, 1),
    channels: [],
  };
  return { 1: first, 2: second, 3: current };
}

/** The engine's plan for the test documents: a channel whose definition differs restarts. */
function planOf(active: ConfigDocument, target: ConfigDocument): ApplyPlan {
  const before = new Map(active.channels.map((channel) => [channel.id, JSON.stringify(channel)]));
  const kept = new Set(target.channels.map((channel) => channel.id));
  return {
    channels: [
      ...target.channels.map((channel) => ({
        id: channel.id,
        label: channel.label,
        change: !before.has(channel.id)
          ? ("added" as const)
          : before.get(channel.id) === JSON.stringify(channel)
            ? ("unchanged" as const)
            : ("changed" as const),
      })),
      ...active.channels
        .filter((channel) => !kept.has(channel.id))
        .map((channel) => ({ id: channel.id, label: channel.label, change: "removed" as const })),
    ],
    endpoints: [],
    alertsChanged: false,
  };
}

export function history(overrides = {}) {
  const docs = documents();
  return stubApi({
    listConfigVersions: async () => VERSIONS,
    getConfigVersion: vi.fn(async (version: number) => docs[version] as ConfigDocument),
    getConfig: vi.fn(async () => docs[3] as ConfigDocument),
    verifyConfigVersion: vi.fn(async (version: number, note: string) => ({
      ...(VERSIONS.find((each) => each.version === version) as ConfigVersionSummary),
      verifications: [{ by: "LAB\\jdoe", at: "2026-11-06T10:00:00Z", note }],
    })),
    previewRollback: vi.fn(async (version: number) => ({
      ok: true,
      issues: [],
      plan: planOf(docs[3] as ConfigDocument, docs[version] as ConfigDocument),
    })),
    rollbackConfig: vi.fn(async (version: number, _reason: string) => ({
      version: 4,
      outcomes:
        version === 2
          ? [
              { channelId: "chem-bridge", outcome: "restarted" as const },
              { channelId: "results", outcome: "removed" as const },
            ]
          : [{ channelId: "chem-bridge", outcome: "unchanged" as const }],
      restartedChannelIds: version === 2 ? ["chem-bridge"] : [],
      unchangedChannelCount: version === 2 ? 0 : 1,
      failures: [],
    })),
    ...overrides,
  });
}
