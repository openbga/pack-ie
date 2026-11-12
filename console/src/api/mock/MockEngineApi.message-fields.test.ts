import { describe, expect, it } from "vitest";
import type { MockEngineApi } from "./index";
import { mockApiFactory, rowsOf } from "./mockTestSupport";

const create = mockApiFactory({ messageCount: 5000, seed: 7 });

describe("parsed fields", () => {
  const fieldsOf = (api: MockEngineApi, id: string) => api.getMessageFields(id);
  const pathOf = (fields: { path: string; value: string }[] | undefined, path: string) =>
    fields?.find((f) => f.path === path)?.value;

  it("lists an HL7 message's fields by the paths filters use, in message order", async () => {
    const api = create();
    const row = (await rowsOf(api, {})).find(
      (r) =>
        r.protocol === "hl7_mllp" && r.origin === "received" && r.messageType?.startsWith("ORU"),
    );
    expect(row).toBeDefined();
    const fields = await fieldsOf(api, row?.id ?? "");
    expect(fields).toMatchObject({ truncated: false, unavailable: null });
    const list = fields?.fields;
    expect(pathOf(list, "MSH.1")).toBeUndefined();
    expect(pathOf(list, "MSH.9.1")).toBe("ORU");
    expect(pathOf(list, "MSH.9.2")).toBe("R01");
    expect(pathOf(list, "MSH.10")).toBe(row?.controlId);
    expect(pathOf(list, "PID.3.1")).toBe(row?.patientId);
    expect(pathOf(list, "OBX.5")).toBeDefined();
    // The second OBX is `OBX[1]`, and nothing is empty.
    expect(pathOf(list, "OBX[1].5") ?? pathOf(list, "OBX[1].3.1")).toBeDefined();
    expect(list?.some((f) => f.value === "")).toBe(false);
    const paths = list?.map((f) => f.path) ?? [];
    expect(paths.indexOf("MSH.9.1")).toBeLessThan(paths.indexOf("PID.3.1"));
    expect(new Set(paths).size).toBe(paths.length);
  });

  it("lists an ASTM message's fields by record", async () => {
    const api = create();
    const row = (await rowsOf(api, {})).find(
      (r) => r.protocol === "astm" && r.origin === "received" && r.state === "routed",
    );
    expect(row).toBeDefined();
    const list = (await fieldsOf(api, row?.id ?? ""))?.fields;
    expect(pathOf(list, "H.2")).toBeUndefined();
    expect(list?.some((f) => /^H\.\d/.test(f.path))).toBe(true);
    expect(list?.some((f) => f.path.startsWith("R.") || f.path.startsWith("O."))).toBe(true);
    expect(list?.some((f) => f.value === "")).toBe(false);
  });

  it("gives a reason in words instead of fields for AU, delimited and invalid messages", async () => {
    const api = create();
    const rows = await rowsOf(api, {});
    const au = rows.find((r) => r.protocol === "au" && r.origin === "received");
    const csv = rows.find((r) => r.protocol === "delimited_file" && r.origin === "received");
    for (const row of [au, csv]) {
      expect(await fieldsOf(api, row?.id ?? "")).toEqual({
        fields: [],
        truncated: false,
        unavailable: "Fields are listed for HL7 and ASTM messages.",
      });
    }
    const invalid = rows.find((r) => r.state === "invalid" && r.protocol === "hl7_mllp");
    if (invalid !== undefined) {
      expect(await fieldsOf(api, invalid.id)).toEqual({
        fields: [],
        truncated: false,
        unavailable: "The original does not parse as HL7; see the message's reason.",
      });
    }
  });

  it("lists at most 500 fields and says when there are more", async () => {
    const api = create();
    const rows = await rowsOf(api, {});
    const lists = await Promise.all(
      rows
        .filter((r) => r.protocol === "hl7_mllp" && r.sizeBytes > 20_000)
        .slice(0, 3)
        .map((r) => fieldsOf(api, r.id)),
    );
    for (const fields of lists) {
      expect(fields?.fields.length).toBeLessThanOrEqual(500);
      if (fields?.truncated) expect(fields.fields).toHaveLength(500);
    }
  });
});
