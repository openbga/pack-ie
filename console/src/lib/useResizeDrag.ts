import { type PointerEvent, useRef, useState } from "react";

interface ResizeOptions {
  size: number;
  axis: "x" | "y";
  direction?: 1 | -1;
  onResize: (size: number) => void;
  onCancel: () => void;
}

/** Shared pointer capture for the navigation and manual splitters. */
export function useResizeDrag({ size, axis, direction = 1, onResize, onCancel }: ResizeOptions) {
  const [dragging, setDragging] = useState(false);
  const drag = useRef<{ id: number; start: number; size: number; cancel: () => void } | null>(null);
  const coordinate = (event: PointerEvent) => (axis === "x" ? event.clientX : event.clientY);

  function onPointerDown(event: PointerEvent<HTMLElement>) {
    if (event.button !== 0 || !event.isPrimary) return;
    event.preventDefault();
    event.currentTarget.focus();
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = { id: event.pointerId, start: coordinate(event), size, cancel: onCancel };
    setDragging(true);
  }

  function onPointerMove(event: PointerEvent<HTMLElement>) {
    const start = drag.current;
    if (!start || start.id !== event.pointerId) return;
    onResize(start.size + direction * (coordinate(event) - start.start));
  }

  function finishDrag(event: PointerEvent<HTMLElement>) {
    const start = drag.current;
    if (!start || start.id !== event.pointerId) return;
    if (event.type === "pointercancel") start.cancel();
    drag.current = null;
    setDragging(false);
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  }

  return {
    dragging,
    pointerProps: {
      onPointerDown,
      onPointerMove,
      onPointerUp: finishDrag,
      onPointerCancel: finishDrag,
      onLostPointerCapture: finishDrag,
    },
  };
}
