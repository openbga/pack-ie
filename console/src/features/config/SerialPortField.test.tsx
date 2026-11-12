import { screen, waitFor, within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";
import { FormProvider, useForm } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";
import type { EngineApi } from "@/api/engine";
import type { SerialPortInfo } from "@/api/types";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderRoutes } from "@/lib/test/router";
import type { EndpointFormValues } from "./endpointForm";
import { SerialPortField } from "./SerialPortField";

function port(name: string, overrides: Partial<SerialPortInfo> = {}): SerialPortInfo {
  return {
    name,
    kind: "usb",
    vid: null,
    pid: null,
    serialNumber: null,
    manufacturer: null,
    product: null,
    usedBy: [],
    ...overrides,
  };
}

const PORTS: SerialPortInfo[] = [
  port("COM1", { kind: "pci", product: "Communications Port" }),
  port("COM4", {
    vid: 0x0403,
    pid: 0x6001,
    manufacturer: "FTDI",
    product: "FT232R USB UART",
    usedBy: [
      {
        endpointId: "chem",
        endpointLabel: "Chemistry serial",
        channelIds: ["a"],
        heldBy: "chem-bridge",
      },
    ],
  }),
  port("COM5", {
    vid: 0x067b,
    pid: 0x2303,
    usedBy: [
      { endpointId: "coag", endpointLabel: "Coag serial", channelIds: ["b"], heldBy: null },
      { endpointId: "other", endpointLabel: "Other serial", channelIds: [], heldBy: null },
    ],
  }),
  port("COM9", { kind: "bluetooth", product: "Standard Serial over Bluetooth link" }),
];

function Harness({ initial }: { initial: string }) {
  const form = useForm<EndpointFormValues>({ defaultValues: { port: initial } as never });
  return (
    <FormProvider {...form}>
      <form onSubmit={form.handleSubmit((values) => (document.title = values.port))}>
        <SerialPortField />
        <p data-testid="value">{form.watch("port")}</p>
        <button type="submit">Save</button>
      </form>
    </FormProvider>
  );
}

const routes = [
  { path: "/new", element: <Harness initial="" /> },
  { path: "/edit/:id", element: <Harness initial="COM4" /> },
  { path: "/missing/:id", element: <Harness initial="COM12" /> },
];

function api(listSerialPorts: () => Promise<SerialPortInfo[]>) {
  return { listSerialPorts: vi.fn(listSerialPorts) } as unknown as EngineApi & {
    listSerialPorts: ReturnType<typeof vi.fn>;
  };
}

const field = () => screen.getByRole("combobox", { name: /^Port/ });
const value = () => screen.getByTestId("value").textContent;
const options = () =>
  within(screen.getByRole("listbox"))
    .getAllByRole("option")
    .map((option) => option.textContent);

/** The field once the ports are listed. */
async function ready() {
  await waitFor(() => expect(field()).toHaveProperty("disabled", false));
}

async function open(user: UserEvent) {
  await ready();
  await user.click(field());
}

describe("SerialPortField", () => {
  it("lists each port with its kind, device text and ids, and what already uses it", async () => {
    const { user } = renderRoutes(routes, "/new", { api: api(async () => PORTS) });
    await open(user);
    expect(options()).toEqual([
      "COM1PCI · Communications Port",
      "COM4USB · FT232R USB UART (FTDI) · 0403:6001used by Chemistry serial · open by chem-bridge",
      "COM5USB · 067b:2303used by Coag serial, Other serial",
      "COM9Bluetooth · Standard Serial over Bluetooth link",
    ]);
    expect(screen.getByText("4 serial ports found on this computer.")).toBeTruthy();
  });

  it("narrows by name or device text and offers the typed name as it is", async () => {
    const { user } = renderRoutes(routes, "/new", { api: api(async () => PORTS) });
    await open(user);
    await user.type(field(), "ftdi");
    expect(options()).toEqual([
      "COM4USB · FT232R USB UART (FTDI) · 0403:6001used by Chemistry serial · open by chem-bridge",
      "Use “ftdi” anywayNot found on this computer",
    ]);
  });

  it("does not mark the endpoint's own port, and names a port that is its own as found", async () => {
    const { user } = renderRoutes(routes, "/edit/chem", { api: api(async () => PORTS) });
    await open(user);
    expect(field()).toHaveProperty("value", "COM4");
    expect(options()[1]).toBe("COM4USB · FT232R USB UART (FTDI) · 0403:6001");
    expect(
      screen.getByText("On this computer: USB · FT232R USB UART (FTDI) · 0403:6001."),
    ).toBeTruthy();
    expect(screen.queryByText(/Two endpoints naming one port/)).toBeNull();
  });

  it("marks every use of the port while an endpoint is created", async () => {
    const { user } = renderRoutes(routes, "/new", { api: api(async () => PORTS) });
    await open(user);
    expect(options()[1]).toContain("used by Chemistry serial");
  });

  it("warns that a port another endpoint names cannot be opened twice, and still allows it", async () => {
    const { user } = renderRoutes(routes, "/edit/chem", { api: api(async () => PORTS) });
    await open(user);
    await user.click(screen.getByRole("option", { name: /^COM5/ }));
    expect(value()).toBe("COM5");
    expect(
      screen.getByText(
        "COM5 is already named by Coag serial, Other serial. Two endpoints naming one port cannot both open it.",
      ),
    ).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(document.title).toBe("COM5"));
  });

  it("keeps a typed name that is not listed and says it was not found", async () => {
    const { user } = renderRoutes(routes, "/new", { api: api(async () => PORTS) });
    await ready();
    await user.type(field(), "/dev/ttyX");
    expect(value()).toBe("/dev/ttyX");
    await user.click(screen.getByRole("option", { name: /^Use “\/dev\/ttyX” anyway/ }));
    expect(value()).toBe("/dev/ttyX");
    expect(field()).toHaveProperty("value", "/dev/ttyX");
    expect(screen.getByText(/\/dev\/ttyX was not found on this computer/)).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(document.title).toBe("/dev/ttyX"));
  });

  it("keeps a saved port that is no longer listed, selected and labelled", async () => {
    const { user } = renderRoutes(routes, "/missing/x", { api: api(async () => PORTS) });
    await open(user);
    expect(field()).toHaveProperty("value", "COM12");
    expect(screen.getByText(/COM12 was not found on this computer/)).toBeTruthy();
    expect(screen.getByRole("option", { selected: true }).textContent).toBe(
      "Use “COM12” anywayNot found on this computer",
    );
  });

  it("asks for a port before saving", async () => {
    const { user } = renderRoutes(routes, "/new", { api: api(async () => PORTS) });
    await ready();
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Enter port.")).toBeTruthy();
    expect(field().getAttribute("aria-required")).toBe("true");
  });

  it("looks for ports with the control disabled", async () => {
    renderRoutes(routes, "/new", { api: api(() => new Promise(() => {})) });
    expect(await screen.findByText("Looking for serial ports…")).toBeTruthy();
    expect(field()).toHaveProperty("disabled", true);
    expect(field().getAttribute("placeholder")).toBe("Looking for serial ports…");
  });

  it("says when the computer has no serial ports, and still takes a name", async () => {
    const { user } = renderRoutes(routes, "/new", { api: api(async () => []) });
    expect(
      await screen.findByText("No serial ports found on this computer. Type a name."),
    ).toBeTruthy();
    await ready();
    await user.click(field());
    expect(
      screen.getByText(/No serial ports found on this computer\. Type a name to use it\./),
    ).toBeTruthy();
    await user.type(field(), "COM12");
    expect(value()).toBe("COM12");
  });

  it("reports a listing that failed, offers Try again, and keeps the form usable", async () => {
    const list = vi
      .fn<() => Promise<SerialPortInfo[]>>()
      .mockRejectedValueOnce(new Error("no enumeration"))
      .mockResolvedValue(PORTS);
    const { user } = renderRoutes(routes, "/new", { api: api(list) });
    expect(await screen.findByText("The engine could not list the serial ports")).toBeTruthy();
    expect(field()).toHaveProperty("disabled", false);
    await user.type(field(), "COM12");
    expect(value()).toBe("COM12");
    await user.keyboard("{Escape}");
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("4 serial ports found on this computer.")).toBeTruthy();
    expect(screen.queryByText("The engine could not list the serial ports")).toBeNull();
    expect(value()).toBe("COM12");
  });

  it("scans again on Refresh, for an adapter plugged in meanwhile", async () => {
    const list = vi
      .fn<() => Promise<SerialPortInfo[]>>()
      .mockResolvedValueOnce([PORTS[0] as SerialPortInfo])
      .mockResolvedValue(PORTS);
    const { user } = renderRoutes(routes, "/new", { api: api(list) });
    expect(await screen.findByText("1 serial port found on this computer.")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Refresh" }));
    expect(await screen.findByText("4 serial ports found on this computer.")).toBeTruthy();
    expect(list).toHaveBeenCalledTimes(2);
  });

  it("has no accessibility violations, open or closed", async () => {
    const { user, container } = renderRoutes(routes, "/edit/chem", { api: api(async () => PORTS) });
    await ready();
    await expectNoA11yViolations(container);
    await user.click(field());
    await screen.findByRole("listbox");
    await expectNoA11yViolations(document.body);
  });
});
