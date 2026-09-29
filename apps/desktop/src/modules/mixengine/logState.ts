import { stripAnsi } from "../../core/ansi";

export type LogEntry =
  | { kind: "line"; stream: "stdout" | "stderr"; at: string; text: string }
  | { kind: "historic"; text: string }
  | { kind: "gap"; missed: number };

/**
 * One SSE frame from `/logs/service/{id}` applied to the existing list.
 *
 * Three variants, no more — an unknown `type` (a variant added in a later version) is ignored
 * rather than thrown, the same rule `daemonState.applyEvent` follows for `/events`.
 */
export function applyLogFrame(entries: LogEntry[], raw: string, maxEntries: number): LogEntry[] {
  let frame: { type?: unknown; stream?: unknown; at?: unknown; text?: unknown; missed?: unknown };
  try {
    frame = JSON.parse(raw) as typeof frame;
  } catch {
    return entries;
  }

  /* Terminal colours are stripped right here — once per line, the only place both log views
     (`Logs`, `ApplyDialog`) pass through — rather than on every render of every visible line. The
     daemon keeping the original is correct; see `core/ansi.ts`. */
  let next: LogEntry | null = null;
  if (frame.type === "line" && typeof frame.text === "string") {
    const stream = frame.stream === "stderr" ? "stderr" : "stdout";
    const at = typeof frame.at === "string" ? frame.at : "";
    next = { kind: "line", stream, at, text: stripAnsi(frame.text) };
  } else if (frame.type === "historic" && typeof frame.text === "string") {
    next = { kind: "historic", text: stripAnsi(frame.text) };
  } else if (frame.type === "gap" && typeof frame.missed === "number") {
    next = { kind: "gap", missed: frame.missed };
  }

  if (next === null) return entries;
  const combined = [...entries, next];
  return combined.length > maxEntries ? combined.slice(combined.length - maxEntries) : combined;
}

export type StreamFilter = "all" | "stdout" | "stderr";

/** How many printed lines each filter would leave. A gap is a marker, not a line, and counts nowhere. */
export interface StreamCounts {
  all: number;
  stdout: number;
  stderr: number;
  /** Lines read back from `current.log`: in `all`, and in neither stream, because none is known. */
  historic: number;
}

export function countStreams(entries: readonly LogEntry[]): StreamCounts {
  const counts: StreamCounts = { all: 0, stdout: 0, stderr: 0, historic: 0 };
  for (const entry of entries) {
    if (entry.kind === "gap") continue;
    counts.all += 1;
    if (entry.kind === "historic") counts.historic += 1;
    else counts[entry.stream] += 1;
  }
  return counts;
}

/**
 * The entries a stream filter leaves on screen.
 *
 * A historic line is not known to be on either stream, so a stream filter hides it rather than
 * claiming it — showing it under both is what made the filter look like it did nothing. A gap stays:
 * it says where lines were lost, whichever stream they were on.
 */
export function filterByStream(entries: readonly LogEntry[], filter: StreamFilter): LogEntry[] {
  if (filter === "all") return [...entries];
  return entries.filter((entry) => entry.kind === "gap" || (entry.kind === "line" && entry.stream === filter));
}
