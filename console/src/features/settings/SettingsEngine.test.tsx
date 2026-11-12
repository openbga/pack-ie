import { screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { mockGridLayout } from "@/lib/test/layout";
import { renderRoutes } from "@/lib/test/router";
import { engineSettings, settingsApi, settingsRoutes } from "./testFixtures";

describe("SettingsPage", () => {
  beforeEach(mockGridLayout);

  it("shows the bootstrap settings read-only with the restart note", async () => {
    renderRoutes(settingsRoutes, "/settings/engine", { api: settingsApi() });
    expect(await screen.findByText("Lab computer")).toBeTruthy();
    expect(
      screen.getByText(/read when the engine starts; changing them needs a restart/),
    ).toBeTruthy();
    expect(screen.getByText("/var/lib/packie/packie.sock")).toBeTruthy();
    expect(screen.getByText("14 days")).toBeTruthy();
    expect(screen.getByText("Operating system service")).toBeTruthy();
    // Only the alert thresholds are editable here.
    expect(await screen.findByRole("textbox", { name: /Delivery queue warning/ })).toBeTruthy();
    // The four thresholds and the note that says why they change.
    expect(screen.getAllByRole("textbox")).toHaveLength(5);
  });

  it("applies changed alert thresholds through validate and apply", async () => {
    const api = settingsApi();
    const { user } = renderRoutes(settingsRoutes, "/settings/engine", { api });
    const depth = await screen.findByRole("textbox", { name: /Delivery queue warning/ });
    await user.clear(depth);
    await user.type(depth, "250");
    await user.click(screen.getByRole("button", { name: "Save thresholds" }));
    // Nothing is applied without a reason.
    expect(await screen.findByText("Enter change note.")).toBeTruthy();
    expect(api.applyConfig).not.toHaveBeenCalled();
    await user.type(
      screen.getByRole("textbox", { name: /^Change note/ }),
      "Queue warning raised after the weekend backlog",
    );
    await user.click(screen.getByRole("button", { name: "Save thresholds" }));
    expect(await screen.findByText("Configuration version 8 applied")).toBeTruthy();
    expect(api.validateConfig).toHaveBeenCalledTimes(1);
    const [document, note] = api.applyConfig.mock.calls[0] ?? [];
    expect(document?.alertThresholds).toEqual({
      endpointDownSeconds: 120,
      queueDepthWarning: 250,
      bufferPressurePercent: 85,
      diskFreeMinMib: 2048,
    });
    expect(document?.version).toBe(7);
    expect(note).toBe("Queue warning raised after the weekend backlog");
  });

  it("checks the format, and puts an engine finding on its field", async () => {
    const api = settingsApi({
      validateConfig: vi.fn(async () => ({
        ok: false,
        issues: [
          {
            severity: "error" as const,
            path: "alertThresholds.diskFreeMinMib",
            message: "Too high for this disk.",
          },
        ],
      })),
    });
    const { user } = renderRoutes(settingsRoutes, "/settings/engine", { api });
    const down = await screen.findByRole("textbox", { name: /Endpoint down after/ });
    await user.clear(down);
    await user.type(down, "0");
    await user.click(screen.getByRole("button", { name: "Save thresholds" }));
    expect(await screen.findByText("Enter a whole number from 1 to 86400.")).toBeTruthy();
    expect(vi.mocked(api.validateConfig)).not.toHaveBeenCalled();
    await user.clear(down);
    await user.type(down, "60");
    await user.type(screen.getByRole("textbox", { name: /^Change note/ }), "Tune");
    await user.click(screen.getByRole("button", { name: "Save thresholds" }));
    const disk = screen.getByRole("textbox", { name: /Disk free minimum/ });
    await waitFor(() => expect(disk.getAttribute("aria-invalid")).toBe("true"));
    expect(screen.getAllByText("Too high for this disk.").length).toBeGreaterThan(0);
    expect(api.applyConfig).not.toHaveBeenCalled();
    expect((down as HTMLInputElement).value).toBe("60");
  });

  it("edits the four alert thresholds with their ranges", async () => {
    const { user } = renderRoutes(settingsRoutes, "/settings/engine", { api: settingsApi() });
    const field = async (name: RegExp) => screen.findByRole("textbox", { name });
    expect(((await field(/^Endpoint down after \(seconds\)/)) as HTMLInputElement).value).toBe(
      "120",
    );
    expect(
      ((await field(/^Delivery queue warning \(deliveries\)/)) as HTMLInputElement).value,
    ).toBe("500");
    expect(((await field(/^Buffer pressure \(percent\)/)) as HTMLInputElement).value).toBe("85");
    expect(((await field(/^Disk free minimum \(MiB\)/)) as HTMLInputElement).value).toBe("2048");
    await user.clear(await field(/^Buffer pressure/));
    await user.type(await field(/^Buffer pressure/), "101");
    await user.clear(await field(/^Disk free minimum/));
    await user.type(await field(/^Disk free minimum/), "0");
    await user.click(screen.getByRole("button", { name: "Save thresholds" }));
    expect(await screen.findByText("Enter a whole number from 1 to 100.")).toBeTruthy();
    expect(screen.getByText("Enter a whole number from 1 to 10000000.")).toBeTruthy();
    // The queue warning may be 0, which turns it off.
    expect(screen.queryByText(/Enter a whole number from 0 to/)).toBeNull();
    await user.clear(await field(/^Delivery queue warning/));
    await user.type(await field(/^Delivery queue warning/), "-1");
    await user.click(screen.getByRole("button", { name: "Save thresholds" }));
    expect(await screen.findByText("Enter a whole number from 0 to 10000000.")).toBeTruthy();
  });

  it("shows the bootstrap file when the engine reports it", async () => {
    renderRoutes(settingsRoutes, "/settings/engine", { api: settingsApi() });
    const term = await screen.findByText("Bootstrap file", { selector: "dt" });
    expect(term.nextElementSibling?.textContent).toBe("/etc/packie/bootstrap.json");
  });

  it("says when the engine does not report its bootstrap settings", async () => {
    const api = settingsApi({
      getEngineSettings: async () =>
        engineSettings({
          stateDir: null,
          logDir: null,
          ipcPath: null,
          bootstrapPath: undefined,
          hosting: null,
          logRetentionDays: null,
          logRingSize: null,
        }),
    });
    renderRoutes(settingsRoutes, "/settings/engine", { api });
    expect(await screen.findByText("Lab computer")).toBeTruthy();
    expect(screen.getAllByText("Not reported by this engine")).toHaveLength(6);
    expect(screen.getByText(/bootstrap file could not be read by the console/)).toBeTruthy();
    expect(screen.getByText(/changing them needs a restart/)).toBeTruthy();
    expect(screen.getByText("node-1")).toBeTruthy();
  });

  it("marks only the fields the engine leaves out, without the bootstrap-file note", async () => {
    const api = settingsApi({
      getEngineSettings: async () => engineSettings({ ipcPath: null, hosting: null }),
    });
    renderRoutes(settingsRoutes, "/settings/engine", { api });
    expect(await screen.findByText("/var/lib/packie")).toBeTruthy();
    expect(screen.getAllByText("Not reported by this engine")).toHaveLength(2);
    expect(screen.queryByText(/bootstrap file could not be read/)).toBeNull();
    expect(screen.getByText("14 days")).toBeTruthy();
  });
});
