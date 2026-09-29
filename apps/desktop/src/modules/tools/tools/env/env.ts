/**
 * Reads and writes environment variables in four forms.
 *
 * The pivot is an **ordered list**, not a `Record`: the order of lines in a `.env` is something
 * the author meant, and shuffling it annoys the next reader.
 *
 * This is the tool users are almost certain to paste DB passwords into. It saves nothing, like
 * every other tool in the module.
 */

export interface EnvPair {
  key: string;
  value: string;
}

/** The position of the closing quote, skipping escaped quotes. `-1` means the value continues onto
 *  the next line. */
function closingIndex(body: string, quote: string): number {
  for (let i = 0; i < body.length; i += 1) {
    if (quote === '"' && body[i] === "\\") {
      i += 1;
      continue;
    }
    if (body[i] === quote) return i;
  }
  return -1;
}

function unescapeDouble(raw: string): string {
  return raw.replace(/\\(.)/g, (_match, ch: string) => {
    if (ch === "n") return "\n";
    if (ch === "t") return "\t";
    if (ch === "r") return "\r";
    return ch;
  });
}

export function parseEnv(text: string): EnvPair[] {
  const pairs: EnvPair[] = [];
  const lines = text.split(/\r?\n/);
  let i = 0;

  while (i < lines.length) {
    let line = lines[i]!.trim();
    i += 1;
    if (line === "" || line.startsWith("#")) continue;
    if (line.startsWith("export ")) line = line.slice(7).trim();

    const eq = line.indexOf("=");
    if (eq === -1) continue;
    const key = line.slice(0, eq).trim();
    if (key === "") continue;

    const rest = line.slice(eq + 1);
    const quote = rest[0];
    if (quote === '"' || quote === "'") {
      let body = rest.slice(1);
      // A quoted value may span several lines.
      while (closingIndex(body, quote) === -1 && i < lines.length) {
        body += `\n${lines[i]!}`;
        i += 1;
      }
      const end = closingIndex(body, quote);
      const raw = end === -1 ? body : body.slice(0, end);
      pairs.push({ key, value: quote === '"' ? unescapeDouble(raw) : raw });
      continue;
    }

    // Unquoted: what follows ` #` is a comment, not part of the value.
    const hash = rest.indexOf(" #");
    pairs.push({ key, value: (hash === -1 ? rest : rest.slice(0, hash)).trim() });
  }

  return pairs;
}

export function parseJsonEnv(text: string): EnvPair[] | null {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    return null;
  }
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  return Object.entries(value as Record<string, unknown>).map(([key, raw]) => ({
    key,
    value:
      raw === null || raw === undefined
        ? ""
        : typeof raw === "object"
          ? JSON.stringify(raw)
          : String(raw),
  }));
}

const NEEDS_QUOTES = /[\s#"'\\]/;

function envValue(value: string): string {
  if (value === "" || !NEEDS_QUOTES.test(value)) return value;
  const escaped = value
    .replace(/\\/g, "\\\\")
    .replace(/"/g, '\\"')
    .replace(/\n/g, "\\n")
    .replace(/\t/g, "\\t");
  return `"${escaped}"`;
}

export function toEnv(pairs: EnvPair[]): string {
  return pairs.map((pair) => `${pair.key}=${envValue(pair.value)}`).join("\n");
}

export function toExport(pairs: EnvPair[]): string {
  return pairs.map((pair) => `export ${pair.key}=${envValue(pair.value)}`).join("\n");
}

export function toJsonEnv(pairs: EnvPair[]): string {
  const record: Record<string, string> = {};
  for (const pair of pairs) record[pair.key] = pair.value;
  return JSON.stringify(record, null, 2);
}

/** The output gets pasted into a real command line, so it is wrapped by shell rules: single
 *  quotes, and a single quote inside has to close the string, be escaped, then reopen it. */
function shellValue(value: string): string {
  return `'${value.replace(/'/g, "'\\''")}'`;
}

export function toDockerArgs(pairs: EnvPair[]): string {
  return pairs.map((pair) => `-e ${pair.key}=${shellValue(pair.value)}`).join(" ");
}
