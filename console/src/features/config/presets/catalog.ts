// The preset catalog: the engine crate's own data file (crates/packie-config/profiles/profiles.json),
// read at build time. A preset is only data: applying one copies explicit values into a connector
// or a destination's mapping, where the operator sees and edits each of them.
import type { Protocol } from "@/api/types";
import catalogJson from "../../../../../crates/packie-config/profiles/profiles.json";

export type ConnectorRole = "source" | "destination";

/** A named bundle of protocol settings: a patch over the connector's current settings. */
export interface ConnectorProfile {
  id: string;
  label: string;
  protocol: Protocol;
  roles: ConnectorRole[];
  summary: string;
  /** Wire keys (`require_end_cr`, `ack_mode`) and their values. */
  settings: Record<string, unknown>;
  evidence: string;
}

/** A ready mapping for one pair of protocols, with an example of what it does. */
export interface MappingPreset {
  id: string;
  label: string;
  from: Protocol;
  to: Protocol;
  summary: string;
  /** Limits and assumptions, shown beside the preset. */
  notes: string[];
  /** The `mapping` object of a destination: `{tables, rules}`. */
  mapping: { tables?: Record<string, Record<string, string>>; rules: unknown[] };
  /** A patch for the destination connector the mapping feeds, such as the HL7 version. */
  destinationSettings: Record<string, unknown>;
  sample: {
    input: string;
    output: string;
    /** The settings of the source connector the example was read with (wire keys). */
    source_settings?: Record<string, unknown>;
  };
  evidence: string;
}

interface CatalogFile {
  format: number;
  connectors: (Omit<ConnectorProfile, never> & { roles: string[] })[];
  mappings: (Omit<MappingPreset, "destinationSettings"> & {
    destination_settings?: Record<string, unknown>;
  })[];
}

const file = catalogJson as unknown as CatalogFile;

const isRole = (role: string): role is ConnectorRole => role === "source" || role === "destination";

export const connectorProfiles: readonly ConnectorProfile[] = file.connectors.map((profile) => ({
  ...profile,
  roles: profile.roles.filter(isRole),
}));

export const mappingPresets: readonly MappingPreset[] = file.mappings.map(
  ({ destination_settings, ...preset }) => ({
    ...preset,
    destinationSettings: destination_settings ?? {},
  }),
);

/** The profiles for a connector's protocol and role, in the catalog's order. */
export function profilesFor(protocol: Protocol | null, role: ConnectorRole): ConnectorProfile[] {
  return connectorProfiles.filter(
    (profile) => profile.protocol === protocol && profile.roles.includes(role),
  );
}

/** The mapping presets that read `from` and write `to`. */
export function presetsFor(from: Protocol | null, to: Protocol | null): MappingPreset[] {
  return mappingPresets.filter((preset) => preset.from === from && preset.to === to);
}
