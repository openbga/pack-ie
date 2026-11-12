import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// jsdom does not scroll elements; emulate it so virtualized lists can scroll in tests.
Object.defineProperty(Element.prototype, "scrollTo", {
  configurable: true,
  value: function scrollTo(this: Element, options?: ScrollToOptions) {
    this.scrollTop = options?.top ?? this.scrollTop;
    this.scrollLeft = options?.left ?? this.scrollLeft;
    this.dispatchEvent(new Event("scroll"));
  },
});

afterEach(() => {
  cleanup();
});
