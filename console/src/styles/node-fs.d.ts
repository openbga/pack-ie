// The UI has no @types/node. This is the one Node function the token test needs, because
// Vitest turns CSS imports (even ?raw) into empty strings.
declare module "node:fs" {
  export function readFileSync(path: string, encoding: "utf8"): string;
}
