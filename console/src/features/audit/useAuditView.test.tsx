import { act, renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { MemoryRouter } from "react-router";
import { describe, expect, it } from "vitest";
import { without } from "@/lib/optionalKeys";
import { useAuditView } from "./useAuditView";

const wrapper = ({ children }: { children: ReactNode }) => (
  <MemoryRouter initialEntries={["/audit"]}>{children}</MemoryRouter>
);

describe("useAuditView", () => {
  it("makes a change from the filters as last asked for, even before the page shows them", () => {
    const { result } = renderHook(() => useAuditView(), { wrapper });
    // Two changes in one go, as when text is typed right after a choice: the page has drawn
    // neither yet, and the second must not undo the first.
    act(() => {
      result.current.setView((current) => ({ ...current, action: "channel_pause" }));
      result.current.setView((current) => ({ ...current, actor: "LAB\\jdoe" }));
    });
    expect(result.current.view).toMatchObject({ action: "channel_pause", actor: "LAB\\jdoe" });
  });

  it("keeps making changes from the page once it has caught up", () => {
    const { result } = renderHook(() => useAuditView(), { wrapper });
    act(() => result.current.setView((current) => ({ ...current, action: "channel_pause" })));
    act(() => result.current.setView((current) => ({ ...current, actor: "LAB\\jdoe" })));
    act(() => result.current.setView((current) => without(current, "action")));
    expect(result.current.view.action).toBeUndefined();
    expect(result.current.view.actor).toBe("LAB\\jdoe");
  });

  it("takes a whole set of filters as given", () => {
    const { result } = renderHook(() => useAuditView(), { wrapper });
    act(() => result.current.setView((current) => ({ ...current, actor: "LAB\\jdoe" })));
    act(() => result.current.setView({ range: "24h" }));
    expect(result.current.view).toEqual({ range: "24h" });
  });
});
