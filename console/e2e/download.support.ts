import type { Download, Locator, Page } from "@playwright/test";

/** Presses `button` and returns the file it makes the browser download. */
export async function downloadFrom(page: Page, button: Locator): Promise<Download> {
  const pending = page.waitForEvent("download");
  await button.click();
  return pending;
}

/** The text of a downloaded file. */
export async function downloadText(download: Download): Promise<string> {
  const stream = await download.createReadStream();
  if (!stream) throw new Error("the download has no content");
  const decoder = new TextDecoder();
  let text = "";
  for await (const chunk of stream) text += decoder.decode(chunk, { stream: true });
  return text + decoder.decode();
}

/**
 * The records of a CSV file as RFC 4180 reads them: quoted cells may hold commas, quotes and line
 * breaks (an exported message body does), and a record ends with CRLF.
 */
export function parseCsv(text: string): string[][] {
  const records: string[][] = [];
  let record: string[] = [];
  // A cell (quoted or plain) and what ends it: a comma, the end of a record, or the end of the text.
  const cell = /("(?:[^"]|"")*"|[^,\r\n]*)(,|\r\n|$)/gy;
  while (cell.lastIndex < text.length) {
    const match = cell.exec(text);
    if (match === null) break;
    const [, value = "", end] = match;
    record.push(value.startsWith('"') ? value.slice(1, -1).replaceAll('""', '"') : value);
    if (end !== ",") {
      records.push(record);
      record = [];
    }
  }
  return records;
}
