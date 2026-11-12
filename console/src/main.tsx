import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "@/app/App";
import { applyAppearance } from "@/lib/appearance";
import "@/styles/global.css";

const root = document.getElementById("root");
if (root === null) {
  throw new Error("The page has no #root element");
}
// The console follows the operating system's light or dark setting.
applyAppearance("system");
createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
