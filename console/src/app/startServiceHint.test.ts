import { describe, expect, it } from "vitest";
import { startServiceHint } from "./startServiceHint";

describe("startServiceHint", () => {
  it.each([
    ["Mozilla/5.0 (Windows NT 10.0; Win64; x64)", "packie service start"],
    [
      "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)",
      'sudo packie --bootstrap "/Library/Application Support',
    ],
    ["Mozilla/5.0 (X11; Linux x86_64)", "sudo packie --bootstrap /etc/packie/bootstrap.json"],
  ])("names the elevated command for %s", (userAgent, start) => {
    const hint = startServiceHint(userAgent);
    expect(hint.command.startsWith(start)).toBe(true);
    expect(hint.why).toContain("administrator");
  });
});
