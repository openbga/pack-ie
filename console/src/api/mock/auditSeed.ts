import type { AuditOrigin } from "../types";
import type { AuditInput } from "./audit";
import type { Rng } from "./prng";

const SEEDED_EVENTS = 420;

export interface Actor {
  name: string;
  uid: string;
}

const CHANNELS = ["ch-br-d100", "ch-msg-sta", "ch-msg-abl", "ch-msg-reflab"] as const;
const messageId = (rng: Rng) => `msg-${rng.int(1, 999)}`;

interface Seed {
  origin: AuditOrigin;
  input: AuditInput;
}

/** One kind of seeded event: its share of the history (out of 100) and how to make one. */
const SEEDS: readonly { weight: number; make: (rng: Rng, index: number) => Seed }[] = [
  {
    weight: 16,
    make: (rng) => ({
      origin: "console",
      input: {
        action: "message_body_view",
        objectType: "message",
        objectId: messageId(rng),
        details: { body: rng.pick(["original", "transformed"]) },
      },
    }),
  },
  {
    weight: 10,
    make: (rng) => ({
      origin: "console",
      input: { action: "message_fields_view", objectType: "message", objectId: messageId(rng) },
    }),
  },
  {
    weight: 12,
    make: (rng) => ({
      origin: "console",
      input: {
        action: "message_search",
        details: { identifier: `SYN-${rng.int(100000, 999999)}`, since: "24h" },
      },
    }),
  },
  {
    weight: 10,
    make: (rng) => ({
      origin: rng.pick(["console", "cli"] as const),
      input: {
        action: rng.pick(["channel_pause", "channel_resume", "channel_restart"] as const),
        objectType: "channel",
        objectId: rng.pick(CHANNELS),
      },
    }),
  },
  {
    weight: 10,
    make: (rng) => ({
      origin: "console",
      input: {
        action: "message_resend",
        objectType: "message",
        objectId: messageId(rng),
        details: { mode: "same_message", reason: "Destination was down for maintenance" },
      },
    }),
  },
  {
    weight: 6,
    make: (rng) => ({
      origin: "console",
      input: {
        action: "delivery_close",
        objectType: "delivery",
        objectId: `dl-${rng.int(1, 999)}`,
        details: { reason: "Confirmed received by phone with the reference lab" },
      },
    }),
  },
  {
    weight: 10,
    make: (rng, index) => ({
      origin: rng.pick(["console", "cli"] as const),
      input: {
        action: "config_apply",
        details: {
          base_version: 40 + (index % 5),
          note: rng.pick([
            "Raise Analyzer 2 baud rate after cable swap",
            "Add LIS destination for hematology",
            "Change request CR-1042: retention to 90 days",
          ]),
        },
      },
    }),
  },
  {
    weight: 6,
    make: (_rng, index) => ({
      origin: "console",
      input: {
        action: "config_verify",
        objectType: "config_version",
        objectId: String(38 + (index % 6)),
        details: { note: "Validation record VR-2026-018" },
      },
    }),
  },
  { weight: 6, make: () => ({ origin: "console", input: { action: "config_validate" } }) },
  {
    weight: 4,
    make: (rng) => ({
      origin: "console",
      input: {
        action: rng.pick(["secret_set", "secret_delete"] as const),
        objectType: "secret",
        objectId: rng.pick(["orders-sftp-key", "reflab-sftp-password"]),
      },
    }),
  },
  {
    weight: 5,
    make: () => ({ origin: "console", input: { action: "audit_verify", objectType: "audit" } }),
  },
  {
    weight: 5,
    make: () => ({
      origin: "console",
      input: { action: "audit_export", objectType: "audit", details: { format: "csv" } },
    }),
  },
];

/** Where each seed kind's share of the rolls ends. */
const THRESHOLDS: number[] = [];
for (const seed of SEEDS) THRESHOLDS.push((THRESHOLDS.at(-1) ?? 0) + seed.weight);

/** Seed events: who did what over the last weeks, in the shapes of docs/spec/audit.md section 3. */
export function seedInputs(
  rng: Rng,
  people: readonly Actor[],
): { who: Actor; origin: AuditOrigin; input: AuditInput }[] {
  return Array.from({ length: SEEDED_EVENTS }, (_, index) => {
    const roll = rng.int(0, (THRESHOLDS.at(-1) ?? 1) - 1);
    const who = rng.pick(people);
    const seed =
      SEEDS[THRESHOLDS.findIndex((end) => roll < end)] ?? (SEEDS[0] as (typeof SEEDS)[0]);
    return { who, ...seed.make(rng, index) };
  });
}
