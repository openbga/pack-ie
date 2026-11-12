/**
 * Hands `text` to the browser as a download: what the demo does where the console would open a
 * save dialog. Nothing happens where there is no page to download into (tests).
 */
export function downloadText(name: string, text: string): void {
  if (typeof URL.createObjectURL !== "function") return;
  const url = URL.createObjectURL(new Blob([text], { type: "text/plain" }));
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 0);
}
