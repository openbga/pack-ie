/** Light, dark or follow the operating system; the page is themed through `data-theme`. */
export type Appearance = "light" | "dark" | "system";

const DARK_QUERY = "(prefers-color-scheme: dark)";

let current: Appearance = "system";

function paint() {
  const dark =
    current === "dark" || (current === "system" && window.matchMedia?.(DARK_QUERY).matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
}

/** Applies an appearance to the page (tokens.css reads `data-theme`). */
export function applyAppearance(appearance: Appearance) {
  current = appearance;
  paint();
}

window.matchMedia?.(DARK_QUERY).addEventListener("change", () => {
  if (current === "system") paint();
});
