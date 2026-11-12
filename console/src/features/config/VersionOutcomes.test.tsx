import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { ChannelOutcome } from "@/api/types";
import { VersionOutcomes } from "./VersionOutcomes";

const FAILED: ChannelOutcome[] = [
  { channelId: "ch-sta", outcome: "failed", reason: "Cannot open COM24: access denied" },
  { channelId: "ch-other", outcome: "unchanged" },
];
const nameOf = (id: string) => (id === "ch-sta" ? "STA Compact results" : id);

describe("VersionOutcomes", () => {
  it("tells the active version's failure in the present tense, with the next step", () => {
    render(<VersionOutcomes outcomes={FAILED} isActive nameOf={nameOf} />);
    expect(
      screen.getByText(
        "The version is active. A channel that failed kept its previous definition; the next apply retries it.",
      ),
    ).toBeTruthy();
    expect(screen.queryByText(/When applied/)).toBeNull();
  });

  it("tells an older version's failure as history, with no call to act", () => {
    render(<VersionOutcomes outcomes={FAILED} isActive={false} nameOf={nameOf} />);
    expect(
      screen.getByText(
        "When applied, this version became active, and a channel that failed kept its previous definition. Later versions have replaced it since.",
      ),
    ).toBeTruthy();
    expect(screen.queryByText(/the next apply retries it/)).toBeNull();
    // What happened to the channel is still listed, with the engine's reason.
    expect(screen.getByText(/Cannot open COM24: access denied/)).toBeTruthy();
    expect(screen.getByText("STA Compact results")).toBeTruthy();
  });

  it("adds no note when nothing failed", () => {
    render(
      <VersionOutcomes
        outcomes={[{ channelId: "ch-sta", outcome: "updated" }]}
        isActive={false}
        nameOf={nameOf}
      />,
    );
    expect(screen.queryByText(/previous definition/)).toBeNull();
  });
});
