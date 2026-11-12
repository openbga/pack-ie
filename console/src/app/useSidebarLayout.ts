import { type KeyboardEvent, useEffect, useState } from "react";

import { useResizeDrag } from "@/lib/useResizeDrag";

const LAYOUT_KEY = "packie.navigation.layout-v1";
const NAV_MIN = 192;
export const NAV_MAX = 360;
export const NAV_DEFAULT = 224;
export const NAV_RAIL = 56;
export const NAV_STEP = 16;

interface Layout {
  width: number;
  collapsed: boolean;
}

function clampWidth(width: number) {
  return Math.min(NAV_MAX, Math.max(NAV_MIN, Math.round(width)));
}

function readLayout(): Layout {
  const fallback = { width: NAV_DEFAULT, collapsed: false };
  try {
    const value: unknown = JSON.parse(localStorage.getItem(LAYOUT_KEY) ?? "null");
    if (typeof value !== "object" || value === null) return fallback;
    return {
      width:
        "width" in value && typeof value.width === "number" && Number.isFinite(value.width)
          ? clampWidth(value.width)
          : NAV_DEFAULT,
      collapsed: "collapsed" in value && value.collapsed === true,
    };
  } catch {
    // Preferences are optional: a damaged or unavailable store must not prevent navigation.
    return fallback;
  }
}

/** Local presentation only. Persist after a drag ends, not on every pointer movement. */
export function useSidebarLayout() {
  const [layout, setLayout] = useState(readLayout);
  const width = layout.collapsed ? NAV_RAIL : layout.width;
  const resize = (next: number) => setLayout({ width: clampWidth(next), collapsed: false });
  const toggle = () => setLayout((last) => ({ ...last, collapsed: !last.collapsed }));

  const { dragging, pointerProps } = useResizeDrag({
    size: width,
    axis: "x",
    onResize: (next) => {
      if (!layout.collapsed || next >= NAV_MIN) resize(next);
    },
    onCancel: () => setLayout(layout),
  });

  useEffect(() => {
    if (dragging) return;
    try {
      localStorage.setItem(LAYOUT_KEY, JSON.stringify(layout));
    } catch {
      // Continue with in-memory preferences when this webview cannot persist them.
    }
  }, [layout, dragging]);

  function onKeyDown(event: KeyboardEvent) {
    if (layout.collapsed && event.key === "ArrowLeft") {
      event.preventDefault();
      return;
    }
    const sizes: Record<string, number> = {
      ArrowLeft: layout.width - NAV_STEP,
      ArrowRight: layout.collapsed ? layout.width : layout.width + NAV_STEP,
      Home: NAV_MIN,
      End: NAV_MAX,
    };
    const next = sizes[event.key];
    if (event.key === "Enter") toggle();
    else if (next !== undefined) resize(next);
    else return;
    event.preventDefault();
  }

  return {
    width,
    collapsed: layout.collapsed,
    dragging,
    resize,
    toggle,
    separatorProps: { onKeyDown, ...pointerProps },
  };
}
