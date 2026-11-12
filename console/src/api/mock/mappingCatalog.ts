import type {
  AuSettings,
  CodeTables,
  ConnectorSettings,
  MappingCondition,
  MappingRule,
  Protocol,
} from "../types";

const TEST_CODES: Record<string, string> = {
  WBC: "6690-2",
  RBC: "789-8",
  HGB: "718-7",
  HCT: "4544-3",
  PLT: "777-3",
  GLU: "2345-7",
  NA: "2951-2",
  K: "2823-3",
  CREA: "2160-0",
};

const ORDER_CODES: Record<string, string> = {
  CBC: "CBC",
  CHEM7: "BMP",
  GLUC: "GLU",
  LIPID: "LIPID",
};

const ANALYZER_TABLE = "analyzer-test-codes";
const ORDER_TABLE = "order-test-codes";

export interface MappingSet {
  rules: MappingRule[];
  codeTables: CodeTables;
}

/** A `when` rule; its branch field is named `then` in the engine's document too. */
export function whenRule(
  id: string,
  condition: MappingCondition,
  thenRules: MappingRule[],
  elseRules: MappingRule[],
): MappingRule {
  // biome-ignore lint/suspicious/noThenProperty: `then` is the rule's field name in runtime.json
  return { id, op: "when", condition, then: thenRules, else: elseRules };
}

/** Hands out the editor keys (`map-1`, `map-2`, ...) of one mapping in order. */
function ids(): () => string {
  let n = 0;
  return () => {
    n += 1;
    return `map-${n}`;
  };
}

function resultsToHl7(next: () => string): MappingRule[] {
  return [
    { id: next(), op: "copy", from: "P.4", to: "PID.3[0].1", default: "", required: true },
    { id: next(), op: "copy", from: "O.3", to: "OBR.3", default: "", required: true },
    {
      id: next(),
      op: "datetime",
      from: "R.13",
      to: "OBR.7",
      input: "yyyyMMddHHmmss",
      output: "yyyyMMddHHmmss",
      fromTimezone: "",
      toTimezone: "",
    },
    { id: next(), op: "constant", value: "LIS", to: "MSH.6" },
    {
      id: next(),
      op: "for_each",
      over: "R",
      into: "OBX",
      rules: [
        { id: next(), op: "sequence", to: "$.1", start: 1 },
        { id: next(), op: "constant", value: "NM", to: "$.2" },
        {
          id: next(),
          op: "lookup",
          from: "$.3",
          table: ANALYZER_TABLE,
          to: "$.3[0].1",
          onMissing: "keep",
          default: "",
          ignoreCase: false,
        },
        { id: next(), op: "copy", from: "$.4", to: "$.5", default: "", required: true },
        { id: next(), op: "copy", from: "$.5", to: "$.6", default: "", required: false },
        whenRule(
          next(),
          { path: "$.7", operator: "exists", value: "", ignoreCase: false },
          [{ id: next(), op: "copy", from: "$.7", to: "$.8", default: "", required: false }],
          [{ id: next(), op: "constant", value: "N", to: "$.8" }],
        ),
      ],
    },
  ];
}

/** ASTM or AU results as one row of a delimited file. */
function resultsToDelimited(next: () => string): MappingRule[] {
  return [
    { id: next(), op: "copy", from: "O.3", to: "col.sample_id", default: "", required: true },
    { id: next(), op: "copy", from: "P.4", to: "col.patient_id", default: "", required: false },
    {
      id: next(),
      op: "concat",
      parts: [{ from: "R.3" }, { text: " = " }, { from: "R.4" }],
      to: "col.test",
    },
    {
      id: next(),
      op: "datetime",
      from: "R.13",
      to: "col.collected_at",
      input: "yyyyMMddHHmmss",
      output: "yyyy-MM-dd HH:mm:ss",
      fromTimezone: "",
      toTimezone: "",
    },
  ];
}

function otherToHl7(source: Protocol, next: () => string): MappingRule[] {
  const delimited = source === "delimited_file";
  return [
    {
      id: next(),
      op: "copy",
      from: delimited ? "col.patient_id" : "PID.3[0].1",
      to: "PID.3[0].1",
      default: "",
      required: false,
    },
    {
      id: next(),
      op: "copy",
      from: delimited ? "col.sample_id" : "OBR.3",
      to: "OBR.3",
      default: "",
      required: false,
    },
    { id: next(), op: "constant", value: "PACKIE", to: "MSH.3" },
    { id: next(), op: "constant", value: "MAINLAB", to: "MSH.4" },
  ];
}

function hl7ToDelimited(next: () => string): MappingRule[] {
  return [
    { id: next(), op: "copy", from: "OBR.3", to: "col.sample_id", default: "", required: true },
    {
      id: next(),
      op: "copy",
      from: "PID.3[0].1",
      to: "col.patient_id",
      default: "",
      required: false,
    },
    {
      id: next(),
      op: "concat",
      parts: [{ from: "OBR.4.1" }, { text: " - " }, { from: "OBR.4.2" }],
      to: "col.test",
    },
    {
      id: next(),
      op: "datetime",
      from: "OBR.7",
      to: "col.collected_at",
      input: "yyyyMMddHHmmss",
      output: "yyyy-MM-dd HH:mm:ss",
      fromTimezone: "",
      toTimezone: "",
    },
  ];
}

function hl7ToInstrument(next: () => string): MappingRule[] {
  return [
    { id: next(), op: "copy", from: "PID.3[0].1", to: "P.4", default: "", required: false },
    { id: next(), op: "copy", from: "OBR.3", to: "O.3", default: "", required: true },
    {
      id: next(),
      op: "lookup",
      from: "OBR.4.1",
      table: ORDER_TABLE,
      to: "O.5",
      onMissing: "error",
      default: "",
      ignoreCase: false,
    },
    whenRule(
      next(),
      { path: "OBR.5", operator: "equals", value: "S", ignoreCase: true },
      [{ id: next(), op: "constant", value: "S", to: "O.6" }],
      [],
    ),
  ];
}

function tablesUsed(rules: MappingRule[], found: Set<string>): Set<string> {
  for (const rule of rules) {
    if (rule.op === "lookup") found.add(rule.table);
    if (rule.op === "for_each") tablesUsed(rule.rules, found);
    if (rule.op === "when") {
      tablesUsed(rule.then, found);
      tablesUsed(rule.else, found);
    }
  }
  return found;
}

const ALL_TABLES: CodeTables = {
  [ANALYZER_TABLE]: TEST_CODES,
  [ORDER_TABLE]: ORDER_CODES,
};

/**
 * A realistic mapping (and the code tables it refers to) for a destination of a channel whose
 * source speaks `source`. Empty when the protocols are the same: the destination then passes the
 * original message through unchanged (docs/spec/message-channels.md §6).
 */
export function mappingsFor(source: Protocol, destination: Protocol): MappingSet {
  if (source === destination) return { rules: [], codeTables: {} };
  const next = ids();
  const fromInstrument = source === "astm" || source === "au";
  let rules: MappingRule[];
  if (destination === "hl7_mllp") {
    rules = fromInstrument ? resultsToHl7(next) : otherToHl7(source, next);
  } else if (destination === "delimited_file") {
    rules = fromInstrument ? resultsToDelimited(next) : hl7ToDelimited(next);
  } else {
    rules = hl7ToInstrument(next);
  }
  const codeTables: CodeTables = {};
  for (const name of tablesUsed(rules, new Set())) {
    const table = ALL_TABLES[name];
    if (table !== undefined) codeTables[name] = { ...table };
  }
  return { rules, codeTables };
}

const SOURCE_COLUMNS = ["patient_id", "sample_id", "test_code", "result", "collected_at"];
const DESTINATION_COLUMNS = ["sample_id", "patient_id", "test", "collected_at"];

/** The JSON column template a delimited connector carries (docs/spec/configuration.md §7.4). */
function delimitedTemplate(role: "source" | "destination"): string {
  const columns = (role === "source" ? SOURCE_COLUMNS : DESTINATION_COLUMNS).map((name) =>
    name === "collected_at"
      ? { name, type: "datetime", format: "yyyy-MM-dd HH:mm:ss" }
      : { name, type: "string" },
  );
  const template =
    role === "source"
      ? { delimiter: ",", header_row: true, columns }
      : {
          delimiter: ",",
          header_row: true,
          file_name: "results-{date}.csv",
          on_exists: "suffix",
          columns,
        };
  return JSON.stringify(template, null, 2);
}

/**
 * The online parameter sheet of a demo AU680 (profile `au640`). Synthetic values: a real
 * instrument's sheet says what its own settings are (docs/spec/configuration.md §7.2).
 */
export function auSettings(): AuSettings {
  return {
    profile: "au640",
    framingStart: [2],
    framingEnd: [3],
    bcc: true,
    useEtb: false,
    maxWireLen: 512,
    framingClass: "b",
    unitNo: false,
    rackDigits: 4,
    sampleIdDigits: 10,
    sex: true,
    age: true,
    month: false,
    patientInfoDigits: [10, 0, 0, 0, 0, 0],
    resultDigits: 6,
    frameTimeoutMs: 1000,
    blockTimeoutMs: 3000,
  };
}

export function connectorSettings(
  protocol: Protocol,
  role: "source" | "destination",
): ConnectorSettings {
  switch (protocol) {
    case "hl7_mllp":
      // What the engine's document gives for a connector that sets nothing else.
      return {
        ackMode: role === "destination" ? "enhanced" : "original",
        hl7Version: "2.5.1",
        requireEndCr: true,
        ackMessageTypes: ["ACK"],
        replyType: "ack",
      };
    case "delimited_file":
      return { templateJson: delimitedTemplate(role) };
    case "astm":
      return {
        astmRole: "computer",
        maxFrameChars: 240,
        enqAttempts: 3,
        busyRetryMs: 10_000,
        sampleIdPath: "O.3",
      };
    case "au":
      return { au: auSettings() };
    case "raw":
      return {};
  }
}
