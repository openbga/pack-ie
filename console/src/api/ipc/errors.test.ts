import { describe, expect, it } from "vitest";
import type { ErrorCode, IpcError, RejectionCode } from "@/ipc/generated";
import { fromCommandError, fromIpcError, fromServiceError } from "./errors";
import type { WireConfig } from "./wire";

function ipc(code: ErrorCode, extra: Partial<IpcError> = {}): IpcError {
  return { code, message: `a ${code} message`, field_errors: [], reason_code: null, ...extra };
}

describe("engine errors", () => {
  it.each([
    ["invalid_config", "validation"],
    ["conflict", "conflict"],
    ["not_found", "not_found"],
    ["not_implemented", "not_implemented"],
    ["unavailable", "unavailable"],
    ["bad_request", "internal"],
    ["internal", "internal"],
    ["rejected", "conflict"],
  ] as const)("%s becomes %s", (code, expected) => {
    expect(fromIpcError(ipc(code))).toEqual({
      code: expected,
      message: `a ${code} message`,
      fieldErrors: [],
      correlationId: "",
    });
  });

  it.each([
    ["not_found", "not_found"],
    ["destination_not_found", "not_found"],
    ["version_conflict", "conflict"],
    ["invalid_config", "validation"],
    ["already_stopped", "conflict"],
    ["duplicate_risk_unconfirmed", "conflict"],
    ["transcript_in_use", "conflict"],
  ] as [RejectionCode, string][])("a rejection for %s reads %s", (reason, expected) => {
    expect(fromIpcError(ipc("rejected", { reason_code: reason })).code).toBe(expected);
  });

  it("translates the paths of field errors and leaves warnings out", () => {
    const error = fromIpcError(
      ipc("invalid_config", {
        field_errors: [
          {
            severity: "error",
            code: "schema",
            path: "endpoints[0].settings.port_name",
            message: "must not be empty",
          },
          {
            severity: "error",
            code: "receiver_required",
            path: "channels[1].connection.receiver",
            message: "required for receiver_then_sender",
          },
          { severity: "error", code: "schema", path: "", message: "bad document" },
          {
            severity: "warning",
            code: "retention_unbounded",
            path: "channels[1].retention",
            message: "keeps everything",
          },
        ],
      }),
    );
    expect(error.fieldErrors).toEqual([
      { field: "endpoints[0].port", message: "must not be empty" },
      { field: "channels[1].pairingReceiver", message: "required for receiver_then_sender" },
      { field: "", message: "bad document" },
    ]);
  });

  it("refines the paths with the document the engine checked", () => {
    const document: WireConfig = {
      channels: [
        {
          channel_id: "c",
          label: "C",
          kind: "message",
          source: { endpoint: "e", protocol: "astm" },
          filters: [
            { path: "X", op: "near" },
            { path: "PID.3", op: "exists" },
          ],
          destinations: [],
        },
      ],
    };
    const error = fromIpcError(
      ipc("invalid_config", {
        field_errors: [
          {
            severity: "error",
            code: "filter_invalid",
            path: "channels[0].filters[1].path",
            message: "bad",
          },
        ],
      }),
      document,
    );
    expect(error.fieldErrors).toEqual([{ field: "channels[0].filters[0].path", message: "bad" }]);
  });
});

describe("command errors", () => {
  it("maps each kind", () => {
    expect(fromCommandError({ kind: "ipc", error: ipc("conflict") }).code).toBe("conflict");
    expect(fromCommandError({ kind: "disconnected", message: "gone" })).toEqual({
      code: "engine_unavailable",
      message: "gone",
      fieldErrors: [],
      correlationId: "",
    });
    expect(fromCommandError({ kind: "internal", message: "timed out" })).toMatchObject({
      code: "internal",
      message: "timed out",
    });
  });

  it("falls back to plain words", () => {
    expect(fromCommandError({ kind: "disconnected", message: " " }).message).toBe(
      "The engine is not running.",
    );
    expect(fromCommandError("command ipc_request not found")).toMatchObject({
      code: "internal",
      message: "The console could not complete the action.",
    });
    expect(fromCommandError(new Error("boom")).code).toBe("internal");
    expect(fromCommandError(null).code).toBe("internal");
  });

  it("passes an engine error through", () => {
    const error = { code: "not_found", message: "x", fieldErrors: [], correlationId: "" } as const;
    expect(fromCommandError(error)).toBe(error);
  });

  it("keeps the text of a failed service start", () => {
    expect(
      fromServiceError({ kind: "internal", message: "not implemented in this build" }),
    ).toEqual({
      code: "unavailable",
      message: "not implemented in this build",
      fieldErrors: [],
      correlationId: "",
    });
    expect(fromServiceError("plain").message).toBe("plain");
    expect(fromServiceError(undefined).code).toBe("unavailable");
  });
});
