/* What a local target sets in its shell, as the form edits it — T205, D9. Plain text in and out,
   because the form holds a `Textarea` and the saved entry holds a map. */

/** `KEY=value` lines into a map, or the 1-based number of the first line that is not one. */
export function parseEnvLines(text: string): { env: Record<string, string> } | { error: number } {
  const env: Record<string, string> = {};
  const lines = text.split(/\r?\n/);
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i].trim();
    if (line === "") continue;
    const at = line.indexOf("=");
    if (at <= 0) return { error: i + 1 };
    env[line.slice(0, at).trim()] = line.slice(at + 1).trim();
  }
  return { env };
}

/** The map back into the lines `parseEnvLines` reads. */
export function formatEnvLines(env: Record<string, string> | undefined): string {
  return Object.entries(env ?? {})
    .map(([key, value]) => `${key}=${value}`)
    .join("\n");
}

/** One directory per line. */
export function parsePathLines(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line !== "");
}
