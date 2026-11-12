import { vi } from "vitest";

/**
 * jsdom has no layout. Gives every scroll container a 320px (10-row) viewport and room to
 * scroll, so virtualized grids render their rows. Call in `beforeEach`; mocks are restored.
 */
export function mockGridLayout() {
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(320);
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(800);
  vi.spyOn(Element.prototype, "clientHeight", "get").mockReturnValue(320);
  vi.spyOn(Element.prototype, "scrollHeight", "get").mockReturnValue(10_000_000);
}
