declare const process: { env: Record<string, string | undefined> };

/** Times render in this computer's zone; tests pin it so their expectations hold anywhere. */
export function setup() {
  process.env.TZ = "UTC";
}
