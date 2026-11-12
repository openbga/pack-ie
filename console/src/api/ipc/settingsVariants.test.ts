import { describe, expect, it } from "vitest";
import { auFromWire, auToWire } from "./auConfig";
import { issueList, settingsFromWire, settingsToWire } from "./connectorConfig";

const ASTM_DEFAULTS = {
  maxFrameChars: 240,
  enqAttempts: 3,
  busyRetryMs: 10_000,
  sampleIdPath: "O.3",
};

describe("instrument variant settings", () => {
  it("reads what the wire has, and the engine's default for the rest", () => {
    expect(settingsFromWire("astm", { role: "instrument", max_frame_chars: 64_000 })).toEqual({
      astmRole: "instrument",
      ...ASTM_DEFAULTS,
      maxFrameChars: 64_000,
    });
    expect(
      settingsFromWire("hl7_mllp", { require_end_cr: false, ack_message_types: ["ACK", "ORL"] }),
    ).toEqual({
      ackMode: "original",
      hl7Version: "2.5.1",
      requireEndCr: false,
      ackMessageTypes: ["ACK", "ORL"],
      replyType: "ack",
    });
  });

  it("writes only what differs from the original, and leaves other keys alone", () => {
    const raw = { role: "computer", ack_timeout_ms: 9000 };
    const base = settingsFromWire("astm", raw);
    const issues = issueList();
    const ui = {
      id: "d",
      endpointId: "e",
      protocol: "astm" as const,
      settings: { ...base, enqAttempts: 7 },
    };
    expect(settingsToWire(ui, raw, base, "x", issues)).toEqual({
      role: "computer",
      ack_timeout_ms: 9000,
      enq_attempts: 7,
    });
    expect(issues.list).toEqual([]);
  });

  it("writes a value that goes back to the default when the original had it set", () => {
    const raw = { enq_attempts: 7 };
    const base = settingsFromWire("astm", raw);
    const ui = {
      id: "d",
      endpointId: "e",
      protocol: "astm" as const,
      settings: { ...base, enqAttempts: 3 },
    };
    expect(settingsToWire(ui, raw, base, "x", issueList())).toEqual({ enq_attempts: 3 });
  });

  it("writes a new connector sparsely: only what is not the default", () => {
    const ui = {
      id: "s",
      endpointId: "e",
      protocol: "hl7_mllp" as const,
      settings: {
        ackMode: "original" as const,
        hl7Version: "2.5.1",
        requireEndCr: false,
        ackMessageTypes: ["ACK"],
        replyType: "order_response" as const,
      },
    };
    expect(settingsToWire(ui, undefined, null, "x", issueList())).toEqual({
      require_end_cr: false,
      reply_type: "order_response",
    });
  });

  it("keeps the AU test code width optional: absent reads as 2 and only a 3 is written", () => {
    const sheet = {
      profile: "au400",
      framing: { start: [2], end: [3], bcc: true, use_etb: false, max_wire_len: 256, class: "b" },
      format: {
        unit_no: true,
        rack_digits: 4,
        sample_id_digits: 4,
        sex: false,
        age: false,
        month: false,
        patient_info_digits: [0, 0, 0, 0, 0, 0],
        result_digits: 6,
      },
      frame_timeout_ms: 3000,
      block_timeout_ms: 9900,
    };
    const base = auFromWire(sheet);
    expect(base).toBeDefined();
    expect(base && "testCodeDigits" in base).toBe(false);
    const next: Record<string, unknown> = JSON.parse(JSON.stringify(sheet));
    auToWire(next, { ...(base as NonNullable<typeof base>), testCodeDigits: 3 }, base);
    expect((next.format as Record<string, unknown>).test_code_digits).toBe(3);
    const again = auFromWire(next);
    expect(again?.testCodeDigits).toBe(3);
    // Back to 2: written explicitly, because the engine's document had a 3.
    const back: Record<string, unknown> = JSON.parse(JSON.stringify(next));
    auToWire(back, { ...(again as NonNullable<typeof again>), testCodeDigits: 2 }, again);
    expect((back.format as Record<string, unknown>).test_code_digits).toBe(2);
    // Unchanged: nothing is added.
    const same: Record<string, unknown> = JSON.parse(JSON.stringify(sheet));
    auToWire(same, base as NonNullable<typeof base>, base);
    expect((same.format as Record<string, unknown>).test_code_digits).toBeUndefined();
  });

  it("keeps the AU block timeout optional: absent is the default and is never written for the operator", () => {
    const sheet = {
      profile: "au400",
      framing: { start: [2], end: [3], bcc: true, use_etb: false, max_wire_len: 256, class: "b" },
      format: {
        unit_no: true,
        rack_digits: 4,
        sample_id_digits: 4,
        sex: false,
        age: false,
        month: false,
        patient_info_digits: [0, 0, 0, 0, 0, 0],
        result_digits: 6,
        test_code_digits: 2,
      },
      frame_timeout_ms: 3000,
    };
    // A sheet without it is still set up, and reads back unchanged.
    const base = auFromWire(sheet);
    expect(base).toBeDefined();
    expect(base && "blockTimeoutMs" in base).toBe(false);
    const unchanged: Record<string, unknown> = JSON.parse(JSON.stringify(sheet));
    auToWire(unchanged, base as NonNullable<typeof base>, base);
    expect(unchanged).toEqual(sheet);
    // Written as a new sheet with nothing set, it adds neither key (the explicit width stays).
    const fresh: Record<string, unknown> = {};
    auToWire(fresh, base as NonNullable<typeof base>, undefined);
    expect(fresh).not.toHaveProperty("block_timeout_ms");
    expect(auFromWire(fresh)).toEqual(base);
    // Set by the operator: written, and read back.
    const set: Record<string, unknown> = JSON.parse(JSON.stringify(sheet));
    auToWire(set, { ...(base as NonNullable<typeof base>), blockTimeoutMs: 1200 }, base);
    expect(set.block_timeout_ms).toBe(1200);
    const again = auFromWire(set);
    expect(again?.blockTimeoutMs).toBe(1200);
    // Cleared: the default is written explicitly, because the document had a value.
    const cleared: Record<string, unknown> = JSON.parse(JSON.stringify(set));
    const { blockTimeoutMs: _was, ...without } = again as NonNullable<typeof again>;
    auToWire(cleared, without, again);
    expect(cleared.block_timeout_ms).toBe(9900);
    // The default typed on a sheet that had none is the same document: nothing is added.
    const typed: Record<string, unknown> = JSON.parse(JSON.stringify(sheet));
    auToWire(typed, { ...(base as NonNullable<typeof base>), blockTimeoutMs: 9900 }, base);
    expect(typed).toEqual(sheet);
  });

  it("reads the sample id path as O.3 when absent, writes only a different one and keeps other keys", () => {
    expect(settingsFromWire("astm", {}).sampleIdPath).toBe("O.3");
    const raw = { role: "computer", sample_id_path: "O.4.3", charset: "ascii" };
    const base = settingsFromWire("astm", raw);
    expect(base.sampleIdPath).toBe("O.4.3");
    const ui = { id: "s", endpointId: "e", protocol: "astm" as const, settings: base };
    expect(settingsToWire(ui, raw, base, "x", issueList())).toEqual(raw);
    const back = { ...ui, settings: { ...base, sampleIdPath: "O.3" } };
    // The original had a value, so the default is written explicitly rather than dropped.
    expect(settingsToWire(back, raw, base, "x", issueList())).toEqual({
      ...raw,
      sample_id_path: "O.3",
    });
    // A new connector at the default writes nothing for it; a variant is written.
    const fresh = {
      id: "s",
      endpointId: "e",
      protocol: "astm" as const,
      settings: { astmRole: "computer" as const, sampleIdPath: "O.3" },
    };
    expect(settingsToWire(fresh, undefined, null, "x", issueList())).toEqual({});
    expect(
      settingsToWire(
        { ...fresh, settings: { ...fresh.settings, sampleIdPath: "O.4.3" } },
        undefined,
        null,
        "x",
        issueList(),
      ),
    ).toEqual({ sample_id_path: "O.4.3" });
  });
});
