import type { ProbeReport } from "@/api/types";

/** A connection test that passed: the report the engine returns for a reachable endpoint. */
export const PASSED_PROBE: ProbeReport = {
  ok: true,
  summary: "lis.example.test:3001 accepted a connection in 14 ms.",
  steps: [
    { name: "resolve", ok: true, detail: "resolved to 10.0.4.21", millis: 12 },
    {
      name: "connect",
      ok: true,
      detail: "connected to 10.0.4.21:3001 in 14 ms, then closed",
      millis: 14,
    },
  ],
};
