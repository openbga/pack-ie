import { describe, expect, it } from "vitest";
import { mockApiFactory, rejection } from "./mockTestSupport";

const create = mockApiFactory({ messageCount: 500, seed: 7, steady: true });

describe("listSerialPorts", () => {
  it("lists the ports of the computer sorted by name, each with what identifies it", async () => {
    const ports = await create().listSerialPorts();
    expect(ports.map((port) => port.name)).toEqual(["COM1", "COM3", "COM4", "COM5", "COM9"]);
    expect(ports.map((port) => port.kind)).toEqual(["pci", "bluetooth", "usb", "usb", "usb"]);
    const ftdi = ports.find((port) => port.name === "COM4");
    expect(ftdi).toMatchObject({
      vid: 0x0403,
      pid: 0x6001,
      manufacturer: "FTDI",
      product: "FT232R USB UART",
    });
    expect(ports.find((port) => port.name === "COM9")).toMatchObject({ vid: 0x1a86, pid: 0x7523 });
    expect(ports.find((port) => port.name === "COM5")).toMatchObject({ vid: 0x067b, pid: 0x2303 });
  });

  it("names the serial endpoints of the active configuration that use each port", async () => {
    const api = create();
    const ports = await api.listSerialPorts();
    const used = Object.fromEntries(
      ports.map((port) => [port.name, port.usedBy.map((use) => use.endpointId)]),
    );
    expect(used).toEqual({
      COM1: [],
      COM3: [],
      COM4: ["ep-a-au5800"],
      COM5: ["ep-a-c503"],
      COM9: ["ep-a-acltop"],
    });
    expect(ports.find((port) => port.name === "COM4")?.usedBy[0]).toMatchObject({
      endpointLabel: "AU5800 chem analyzer (COM4)",
      channelIds: ["ch-br-au5800"],
    });
  });

  it("reports the channel holding the port open, from the running state of the channels", async () => {
    const api = create();
    const snapshot = await api.getSnapshot();
    const ports = await api.listSerialPorts();
    const heldBy = (name: string) =>
      ports.find((port) => port.name === name)?.usedBy.map((use) => use.heldBy);
    // A running channel holds its port; one that failed to open does not.
    expect(snapshot.channels.find((channel) => channel.id === "ch-br-au5800")?.state).toBe(
      "running",
    );
    expect(heldBy("COM4")).toEqual(["ch-br-au5800"]);
    const acltop = snapshot.channels.find((channel) => channel.id === "ch-br-acltop");
    expect(acltop?.endpoints[0]?.state).toBe("error");
    expect(heldBy("COM9")).toEqual([null]);
  });

  it("follows the live configuration: a port edited and applied moves its marker, without regard to case", async () => {
    const api = create();
    const doc = await api.getConfig();
    const endpoint = doc.endpoints.find((each) => each.id === "ep-a-au5800");
    if (endpoint?.kind !== "serial") throw new Error("expected a serial endpoint");
    endpoint.port = "com1";
    const result = await api.applyConfig(doc, "move the analyzer");
    expect(result.version).toBeGreaterThan(doc.version);
    const ports = await api.listSerialPorts();
    const used = (name: string) =>
      ports.find((port) => port.name === name)?.usedBy.map((use) => use.endpointId);
    expect(used("COM4")).toEqual([]);
    expect(used("COM1")).toEqual(["ep-a-au5800"]);
  });

  it("lists the same ports with no use on a fresh install", async () => {
    const ports = await create({ empty: true }).listSerialPorts();
    expect(ports).toHaveLength(5);
    expect(ports.every((port) => port.usedBy.length === 0)).toBe(true);
  });

  it("can be made to fail, as the engine does when it cannot enumerate ports", async () => {
    const api = create();
    api.failSerialPorts(true);
    expect((await rejection(api.listSerialPorts())).code).toBe("internal");
    api.failSerialPorts(false);
    expect(await api.listSerialPorts()).toHaveLength(5);
  });
});
