import { screen, waitFor, within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { fillTcpClient, openEndpoint, openNewEndpoint, pick } from "./testActions";
import { sampleConfig, sampleSftp, stubApi, validationFails } from "./testFixtures";

const GLOB_HELP = "Matches file names; `*` and `?` only, for example `*.csv`.";
const GLOB_REFUSAL =
  "Use only * and ? as wildcards, for example *.csv. A file name has no folders or brackets.";

/** Validates and returns the endpoint the engine was asked to check, the last of the document. */
async function lastDocument(user: UserEvent, api: ReturnType<typeof stubApi>) {
  await user.click(screen.getByRole("button", { name: "Validate" }));
  await waitFor(() => expect(api.validateConfig).toHaveBeenCalled());
  return api.validateConfig.mock.calls.at(-1)?.[0].endpoints.at(-1);
}

describe("EndpointEditPage: folder file name pattern", () => {
  async function openFolder() {
    const rendered = await openNewEndpoint(stubApi());
    await pick(rendered.user, document.body, "Kind", "Folder");
    await rendered.user.type(screen.getByRole("textbox", { name: /^Label/ }), "Drop folder");
    await rendered.user.type(screen.getByRole("textbox", { name: /^Folder path/ }), "/data/drop");
    return rendered;
  }

  it("is one glob: named, explained and saved as typed, commas and all", async () => {
    const { api, user } = await openFolder();
    expect(screen.queryByRole("textbox", { name: /^File pattern/ })).toBeNull();
    const pattern = screen.getByRole("textbox", { name: /^File name pattern/ });
    expect((pattern as HTMLInputElement).value).toBe("*");
    expect(screen.getByText(GLOB_HELP)).toBeTruthy();
    await user.clear(pattern);
    await user.type(pattern, "result,1?.csv");
    expect(await lastDocument(user, api)).toMatchObject({
      kind: "folder",
      filePattern: "result,1?.csv",
    });
  });

  it.each(["", "in/*.csv", "[ab]*.csv", "{a,b}.csv"])(
    "refuses the pattern %j before calling the engine",
    async (typed) => {
      const { api, user } = await openFolder();
      const pattern = screen.getByRole("textbox", { name: /^File name pattern/ });
      await user.clear(pattern);
      // user-event reads [ and { as the start of a key name; doubled they are typed.
      if (typed !== "") await user.type(pattern, typed.replace(/[[{]/g, "$&$&"));
      await user.click(screen.getByRole("button", { name: "Validate" }));
      expect(
        await screen.findByText(typed === "" ? "Enter file name pattern." : GLOB_REFUSAL),
      ).toBeTruthy();
      expect(api.validateConfig).not.toHaveBeenCalled();
    },
  );
});

describe("EndpointEditPage: connection settings", () => {
  async function openListener(user: UserEvent) {
    await pick(user, document.body, "Kind", "TCP listener");
    await user.type(screen.getByRole("textbox", { name: /^Label/ }), "Instrument in");
    await user.type(screen.getByRole("textbox", { name: /^Port/ }), "5002");
    const bind = screen.getByRole("textbox", { name: /^Bind address/ });
    await user.clear(bind);
    await user.type(bind, "10.0.4.5");
  }

  it("takes the idle timeout of a TCP endpoint in seconds, 0 meaning off", async () => {
    const { api, user } = await openNewEndpoint(stubApi());
    await fillTcpClient(user);
    const idle = screen.getByRole("textbox", { name: /^Idle timeout \(seconds\)/ });
    expect((idle as HTMLInputElement).value).toBe("0");
    expect(screen.getByText(/0 turns it off/)).toBeTruthy();
    await user.clear(idle);
    await user.type(idle, "90");
    expect(await lastDocument(user, api)).toMatchObject({
      kind: "tcp_client",
      idleTimeoutSeconds: 90,
    });
  });

  it("explains the allow-any-peer choice and shows the engine's allowlist finding", async () => {
    const api = stubApi(
      validationFails({
        severity: "error",
        path: "endpoints[3].allowlist",
        message: "A listener on the network needs an allowlist or allow any peer.",
      }),
    );
    const { user } = await openNewEndpoint(api);
    await openListener(user);
    const any = screen.getByRole("checkbox", { name: "Allow any peer to connect" });
    expect((any as HTMLInputElement).checked).toBe(false);
    expect(screen.getByText(/this box is ignored/)).toBeTruthy();
    expect(screen.getByText(/The engine rejects a network listener with neither/)).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Validate" }));
    const list = screen.getByRole("group", { name: "Peer allowlist" });
    expect(
      await within(list).findByText(
        "A listener on the network needs an allowlist or allow any peer.",
      ),
    ).toBeTruthy();
    await user.click(any);
    expect(
      screen.getByText(/Any host that can reach this address and port may connect/),
    ).toBeTruthy();
    expect(screen.queryByText(/The engine rejects a network listener with neither/)).toBeNull();
  });

  it("saves the allowlist choice with the endpoint", async () => {
    const { api, user } = await openNewEndpoint(stubApi());
    await openListener(user);
    await user.click(screen.getByRole("checkbox", { name: "Allow any peer to connect" }));
    expect(await lastDocument(user, api)).toMatchObject({
      kind: "tcp_listener",
      bindAddress: "10.0.4.5",
      allowlist: [],
      allowAnyPeer: true,
      idleTimeoutSeconds: 0,
    });
  });

  it("offers the engine's parity and flow control choices", async () => {
    const { user } = await openEndpoint("chem-serial", "Chemistry serial", stubApi());
    await user.click(screen.getByRole("button", { name: /Parity/ }));
    expect(screen.getAllByRole("option").map((option) => option.textContent)).toEqual([
      "None",
      "Even",
      "Odd",
    ]);
    await user.keyboard("{Escape}");
    await user.click(screen.getByRole("button", { name: /Flow control/ }));
    expect(screen.getAllByRole("option").map((option) => option.textContent)).toEqual([
      "None",
      "Hardware (RTS/CTS)",
      "Software (XON/XOFF)",
    ]);
  });

  it("asks for the secret of the chosen SFTP authentication and a well-formed host key fingerprint", async () => {
    const { api, user } = await openNewEndpoint(stubApi());
    await pick(user, document.body, "Kind", "SFTP");
    await user.type(screen.getByRole("textbox", { name: /^Label/ }), "Partner");
    await user.type(screen.getByRole("textbox", { name: /^Host$/ }), "sftp.example.test");
    await user.type(screen.getByRole("textbox", { name: /^User name/ }), "labxfer");
    await user.type(screen.getByRole("textbox", { name: /^Remote path/ }), "/outbound");
    expect(screen.getByRole("textbox", { name: /^Password secret name/ })).toBeTruthy();
    expect(screen.queryByRole("textbox", { name: /^Passphrase secret name/ })).toBeNull();
    await pick(user, document.body, "Authentication", "Private key");
    expect(screen.queryByRole("textbox", { name: /^Password secret name/ })).toBeNull();
    await user.type(
      screen.getByRole("textbox", { name: /^Private key secret name/ }),
      "partner-key",
    );
    await user.type(
      screen.getByRole("textbox", { name: /^Passphrase secret name/ }),
      "partner-passphrase",
    );
    expect(screen.getByText(/The value is stored under this name in Settings/)).toBeTruthy();
    await user.type(screen.getByRole("textbox", { name: /^Host key fingerprint/ }), "MD5:aa:bb");
    await user.click(screen.getByRole("button", { name: "Save and apply" }));
    expect(await screen.findByText("Enter SHA256: followed by 43 base64 characters.")).toBeTruthy();
    expect(api.validateConfig).not.toHaveBeenCalled();
    const fingerprint = screen.getByRole("textbox", { name: /^Host key fingerprint/ });
    await user.clear(fingerprint);
    await user.type(fingerprint, "SHA256:uNiVztksCsDhcc0u9e8BujQXVUpKZIDTMczCvj3tD2s");
    expect(await lastDocument(user, api)).toMatchObject({
      kind: "sftp",
      id: "partner",
      authMethod: "private_key",
      secretName: "partner-key",
      passphraseSecretName: "partner-passphrase",
      hostKeyFingerprint: "SHA256:uNiVztksCsDhcc0u9e8BujQXVUpKZIDTMczCvj3tD2s",
      port: 22,
      remotePath: "/outbound",
    });
  });

  it("names the SFTP file pattern like the folder's, defaults it to * and refuses a path in it", async () => {
    const { api, user } = await openNewEndpoint(stubApi());
    await pick(user, document.body, "Kind", "SFTP");
    const pattern = screen.getByRole("textbox", { name: /^File name pattern/ });
    expect((pattern as HTMLInputElement).value).toBe("*");
    expect(screen.getByText(GLOB_HELP)).toBeTruthy();
    await user.clear(pattern);
    await user.type(pattern, "in/*.csv");
    await user.click(screen.getByRole("button", { name: "Validate" }));
    expect(await screen.findByText(GLOB_REFUSAL)).toBeTruthy();
    expect(api.validateConfig).not.toHaveBeenCalled();
  });

  it("keeps the SFTP file pattern of an existing endpoint and saves a change to it", async () => {
    const doc = sampleConfig();
    doc.endpoints.push({ ...sampleSftp(), filePattern: "*.hl7" } as never);
    const api = stubApi({ getConfig: async () => doc });
    const { user } = await openEndpoint("partner-sftp", "Partner SFTP", api);
    const pattern = screen.getByRole("textbox", { name: /^File name pattern/ });
    expect((pattern as HTMLInputElement).value).toBe("*.hl7");
    await user.clear(pattern);
    await user.type(pattern, "ORU_*.csv");
    expect(await lastDocument(user, api)).toMatchObject({ kind: "sftp", filePattern: "ORU_*.csv" });
  });

  it("edits an SFTP endpoint, dropping the passphrase when the password method is chosen", async () => {
    const doc = sampleConfig();
    doc.endpoints.push({
      ...sampleSftp(),
      authMethod: "private_key",
      secretName: "partner-key",
      passphraseSecretName: "partner-passphrase",
    } as never);
    const api = stubApi({ getConfig: async () => doc });
    const { user } = await openEndpoint("partner-sftp", "Partner SFTP", api);
    expect(
      (screen.getByRole("textbox", { name: /^Passphrase secret name/ }) as HTMLInputElement).value,
    ).toBe("partner-passphrase");
    await pick(user, document.body, "Authentication", "Password");
    await user.clear(screen.getByRole("textbox", { name: /^Password secret name/ }));
    await user.type(
      screen.getByRole("textbox", { name: /^Password secret name/ }),
      "partner-password",
    );
    expect(await lastDocument(user, api)).toMatchObject({
      authMethod: "password",
      secretName: "partner-password",
      passphraseSecretName: "",
    });
  });
});
