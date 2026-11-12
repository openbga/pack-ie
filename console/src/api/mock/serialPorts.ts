import type { SerialPortInfo, SerialPortUse } from "../types";
import { channelsUsing } from "./endpointInfo";
import type { MockWorld } from "./world";

type Hardware = Omit<SerialPortInfo, "usedBy">;

/** What the mock computer's operating system reports, whatever the configuration says. */
const HARDWARE: Hardware[] = [
  {
    name: "COM1",
    kind: "pci",
    vid: null,
    pid: null,
    serialNumber: null,
    manufacturer: null,
    product: "Communications Port",
  },
  {
    name: "COM3",
    kind: "bluetooth",
    vid: null,
    pid: null,
    serialNumber: null,
    manufacturer: null,
    product: "Standard Serial over Bluetooth link",
  },
  {
    name: "COM4",
    kind: "usb",
    vid: 0x0403,
    pid: 0x6001,
    serialNumber: "A50285BI",
    manufacturer: "FTDI",
    product: "FT232R USB UART",
  },
  {
    name: "COM5",
    kind: "usb",
    vid: 0x067b,
    pid: 0x2303,
    serialNumber: null,
    manufacturer: "Prolific Technology Inc.",
    product: "USB-Serial Controller",
  },
  {
    name: "COM9",
    kind: "usb",
    vid: 0x1a86,
    pid: 0x7523,
    serialNumber: null,
    manufacturer: "wch.cn",
    product: "USB-SERIAL CH340",
  },
];

/** Ports are named alike on the mock computer (Windows): names compare without case. */
const sameName = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();

/**
 * The serial ports of the mock computer, sorted by name, each with the serial endpoints of
 * the active configuration naming it and, per endpoint, the channel holding the port open now.
 */
export function serialPortsOf(world: MockWorld): SerialPortInfo[] {
  const document = world.config.live;
  const channels = world.snapshot().channels;
  const holder = (channelId: string, endpointId: string) => {
    const side = channels
      .find((channel) => channel.id === channelId)
      ?.endpoints.find((ref) => ref.endpointId === endpointId);
    return side?.state === "connected" || side?.state === "listening";
  };
  return HARDWARE.map((port) => {
    const usedBy: SerialPortUse[] = document.endpoints.flatMap((endpoint) => {
      if (endpoint.kind !== "serial" || !sameName(endpoint.port, port.name)) return [];
      const channelIds = channelsUsing(document, endpoint.id).map((channel) => channel.id);
      return [
        {
          endpointId: endpoint.id,
          endpointLabel: endpoint.label,
          channelIds,
          heldBy: channelIds.find((id) => holder(id, endpoint.id)) ?? null,
        },
      ];
    });
    return { ...port, usedBy };
  }).sort((a, b) => a.name.localeCompare(b.name, "en", { numeric: true }));
}
