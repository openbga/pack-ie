import { describe, expect, it } from "vitest";
import type { MessageChannelConfig } from "../types";
import {
  at,
  check,
  destinationOf,
  endpointIndex,
  messageChannelAt,
  messageIndex,
  mockApiFactory,
  patchChannel,
  patchEndpoint,
} from "./mockTestSupport";

const create = mockApiFactory({ messageCount: 500, seed: 7 });

describe("the mock's own documents", () => {
  it("carry for_each with sequence and a lookup with its code table on the destination", async () => {
    const doc = await create().getConfig();
    const channel = messageChannelAt(doc, messageIndex(doc));
    const destination = destinationOf(channel);
    const loop = destination.mappings.find((m) => m.op === "for_each");
    expect(loop?.op === "for_each" && loop.rules.map((r) => r.op)).toEqual(
      expect.arrayContaining(["sequence", "lookup", "when"]),
    );
    expect(Object.keys(destination.codeTables)).toContain("analyzer-test-codes");
    expect(destination.label).toBeTruthy();
    expect(
      JSON.parse(
        messageChannelAt(doc, messageIndex(doc, "ch-msg-qc")).source.settings.templateJson ?? "",
      ),
    ).toBeTypeOf("object");
  });
});

describe("endpoint rules", () => {
  it("rejects a network listener without allowlist unless any peer is allowed", async () => {
    const api = create();
    const doc = await api.getConfig();
    const index = endpointIndex(doc, (e) => e.kind === "tcp_listener");
    patchEndpoint(doc, index, { allowlist: [], allowAnyPeer: false });
    expect(at(await check(doc, api), "error")).toContain(`endpoints[${index}].allowlist`);

    patchEndpoint(doc, index, { allowAnyPeer: true });
    const allowed = await check(doc, api);
    expect(at(allowed, "error")).toEqual([]);
    expect(at(allowed, "warning")).toContain(`endpoints[${index}].allowAnyPeer`);
  });

  it("accepts a loopback listener with an empty allowlist", async () => {
    const api = create();
    const doc = await api.getConfig();
    const index = endpointIndex(doc, (e) => e.kind === "tcp_listener");
    patchEndpoint(doc, index, { allowlist: [], allowAnyPeer: false, bindAddress: "127.0.0.1" });
    const issues = await check(doc, api);
    expect(at(issues, "error")).toEqual([]);
    expect(at(issues, "warning")).not.toContain(`endpoints[${index}].allowAnyPeer`);
  });

  it("rejects allowlist entries that are not addresses or CIDR ranges", async () => {
    const api = create();
    const doc = await api.getConfig();
    const index = endpointIndex(doc, (e) => e.kind === "tcp_listener");
    patchEndpoint(doc, index, {
      allowlist: ["10.0.4.0/24", "10.0.4.300", "lab-pc", "10.0.0.0/40"],
    });
    const errors = at(await check(doc, api), "error");
    expect(errors).toEqual([
      `endpoints[${index}].allowlist[1]`,
      `endpoints[${index}].allowlist[2]`,
      `endpoints[${index}].allowlist[3]`,
    ]);
  });

  it("validates the idle timeout of TCP endpoints", async () => {
    const api = create();
    const doc = await api.getConfig();
    const index = endpointIndex(doc, (e) => e.kind === "tcp_client");
    patchEndpoint(doc, index, { idleTimeoutSeconds: -1 });
    expect(at(await check(doc, api), "error")).toContain(`endpoints[${index}].idleTimeoutSeconds`);
  });

  it("requires a well-formed host key fingerprint for SFTP", async () => {
    const api = create();
    const doc = await api.getConfig();
    const index = endpointIndex(doc, (e) => e.kind === "sftp");
    expect(at(await check(doc, api), "error")).toEqual([]);
    for (const bad of ["", "MD5:aa:bb", "SHA256:short", `SHA256:${"!".repeat(43)}`]) {
      patchEndpoint(doc, index, { hostKeyFingerprint: bad });
      expect(at(await check(doc, api), "error")).toContain(
        `endpoints[${index}].hostKeyFingerprint`,
      );
    }
  });

  it("accepts a file name glob for folders and SFTP, and refuses folders, brackets and blanks", async () => {
    const api = create();
    const doc = await api.getConfig();
    for (const kind of ["folder", "sftp"] as const) {
      const index = endpointIndex(doc, (e) => e.kind === kind);
      patchEndpoint(doc, index, { filePattern: "result?.csv" });
      expect(at(await check(doc, api), "error")).toEqual([]);
      for (const bad of ["", "in/*.csv", "in\\*.csv", "[ab]*", "{a,b}"]) {
        patchEndpoint(doc, index, { filePattern: bad });
        expect(at(await check(doc, api), "error")).toContain(`endpoints[${index}].filePattern`);
      }
      patchEndpoint(doc, index, { filePattern: "*" });
    }
  });

  it("requires the secret of the chosen auth method and warns about unknown ones", async () => {
    const api = create();
    const doc = await api.getConfig();
    const index = endpointIndex(doc, (e) => e.kind === "sftp");
    patchEndpoint(doc, index, {
      authMethod: "private_key",
      secretName: "",
      passphraseSecretName: "no-such-passphrase",
    });
    const issues = await check(doc, api);
    expect(at(issues, "error")).toContain(`endpoints[${index}].secretName`);
    expect(at(issues, "warning")).toContain(`endpoints[${index}].passphraseSecretName`);
    patchEndpoint(doc, index, { authMethod: "password", secretName: "no-such-secret" });
    expect(at(await check(doc, api), "warning")).toContain(`endpoints[${index}].secretName`);
  });
});

describe("bridge rules", () => {
  async function bridge() {
    const api = create();
    const doc = await api.getConfig();
    const index = doc.channels.findIndex((c) => c.id === "ch-br-c503");
    return { api, doc, index };
  }

  it("requires a receiver exactly for receiver_then_sender", async () => {
    const { api, doc, index } = await bridge();
    patchChannel(doc, index, { pairing: "receiver_then_sender", pairingReceiver: null });
    expect(at(await check(doc, api), "error")).toContain(`channels[${index}].pairingReceiver`);
    patchChannel(doc, index, { pairingReceiver: "b" });
    expect(at(await check(doc, api), "error")).toEqual([]);
    patchChannel(doc, index, { pairing: "parallel", pairingReceiver: "a" });
    expect(at(await check(doc, api), "error")).toContain(`channels[${index}].pairingReceiver`);
  });

  it("needs a listener for wait_for_listener_then_pair", async () => {
    const { api, doc, index } = await bridge();
    patchChannel(doc, index, { pairing: "wait_for_listener_then_pair" });
    expect(at(await check(doc, api), "error")).toContain(`channels[${index}].pairing`);
  });

  it("allows retrying forever but not zero attempts", async () => {
    const { api, doc, index } = await bridge();
    const channel = doc.channels[index];
    if (channel?.kind !== "bridge") throw new Error("expected a bridge");
    patchChannel(doc, index, { retry: { ...channel.retry, maxAttempts: null } });
    expect(at(await check(doc, api), "error")).toEqual([]);
    patchChannel(doc, index, { retry: { ...channel.retry, maxAttempts: 0 } });
    expect(at(await check(doc, api), "error")).toContain(`channels[${index}].retry.maxAttempts`);
  });
});

describe("destination rules", () => {
  it("requires a label of 1 to 100 characters and a number of attempts", async () => {
    const api = create();
    const doc = await api.getConfig();
    const index = messageIndex(doc);
    const channel = messageChannelAt(doc, index);
    const [first] = channel.destinations;
    if (first === undefined) throw new Error("expected a destination");
    patchChannel(doc, index, {
      destinations: [{ ...first, label: " ", retry: { ...first.retry, maxAttempts: null } }],
    });
    let errors = at(await check(doc, api), "error");
    expect(errors).toContain(`channels[${index}].destinations[0].label`);
    expect(errors).toContain(`channels[${index}].destinations[0].retry.maxAttempts`);
    patchChannel(doc, index, { destinations: [{ ...first, label: "x".repeat(101) }] });
    errors = at(await check(doc, api), "error");
    expect(errors).toContain(`channels[${index}].destinations[0].label`);
  });
});

describe("delimited connector template", () => {
  async function templateIssues(templateJson: string | undefined) {
    const api = create();
    const doc = await api.getConfig();
    const index = messageIndex(doc, "ch-msg-qc");
    const channel = messageChannelAt(doc, index);
    patchChannel(doc, index, { source: { ...channel.source, settings: { templateJson } } });
    return { issues: await check(doc, api), index };
  }

  it("accepts a JSON object", async () => {
    const { issues } = await templateIssues('{"columns":[]}');
    expect(at(issues, "error")).toEqual([]);
  });

  it.each([
    ["broken JSON", "{ nope"],
    ["a JSON array", "[1, 2]"],
    ["a JSON string", '"x"'],
    ["empty text", "  "],
  ])("rejects %s", async (_name, text) => {
    const { issues, index } = await templateIssues(text);
    expect(at(issues, "error")).toEqual([`channels[${index}].source.settings.templateJson`]);
  });

  it("rejects a missing template", async () => {
    const { issues, index } = await templateIssues(undefined);
    expect(at(issues, "error")).toContain(`channels[${index}].source.settings.templateJson`);
  });
});

describe("filter rules", () => {
  async function filterIssues(filter: Partial<MessageChannelConfig["filters"][number]>) {
    const api = create();
    const doc = await api.getConfig();
    const index = messageIndex(doc);
    patchChannel(doc, index, {
      filters: [
        { id: "f", path: "R.3", operator: "equals", value: "x", ignoreCase: false, ...filter },
      ],
    });
    return { issues: await check(doc, api), index };
  }

  it("needs a value except for exists and not_exists", async () => {
    let { issues, index } = await filterIssues({ value: "" });
    expect(at(issues, "error")).toEqual([`channels[${index}].filters[0].value`]);
    ({ issues } = await filterIssues({ operator: "exists", value: "" }));
    expect(at(issues, "error")).toEqual([]);
    ({ issues } = await filterIssues({ operator: "not_exists", value: "" }));
    expect(at(issues, "error")).toEqual([]);
  });

  it("needs at least one entry for in and not_in", async () => {
    let { issues } = await filterIssues({ operator: "in", value: " , " });
    expect(at(issues, "error")).toHaveLength(1);
    ({ issues } = await filterIssues({ operator: "not_in", value: "A, B" }));
    expect(at(issues, "error")).toEqual([]);
  });

  it("checks regular expressions and warns about non-numeric comparisons", async () => {
    let { issues } = await filterIssues({ operator: "matches", value: "(" });
    expect(at(issues, "error")).toHaveLength(1);
    ({ issues } = await filterIssues({ operator: "matches", value: "x".repeat(201) }));
    expect(at(issues, "error")).toHaveLength(1);
    ({ issues } = await filterIssues({ operator: "gt", value: "abc" }));
    expect(at(issues, "error")).toEqual([]);
    expect(at(issues, "warning")).toHaveLength(1);
  });

  it("checks the path syntax", async () => {
    for (const path of ["", "PID..3", "PID 3", "$.3"]) {
      const { issues, index } = await filterIssues({ path });
      expect(at(issues, "error")).toEqual([`channels[${index}].filters[0].path`]);
    }
    for (const path of ["PID.3[0].1", "OBX[2].5", "R.3", "col.sample_id"]) {
      expect(at((await filterIssues({ path })).issues, "error")).toEqual([]);
    }
  });
});
