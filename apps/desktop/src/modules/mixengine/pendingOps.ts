/**
 * The operations waiting for administrator rights, read out of an event or out of
 * `elevation.status`.
 *
 * `elevation_required` carries **every** waiting operation, oldest first, each with exactly what
 * it will change. The UI shows that list and only then calls `elevation.grant`, which raises
 * exactly one prompt for the whole batch. Declining is an outcome the API models, not an error;
 * `elevation.drop` is the way out.
 *
 * The daemon **never** raises the prompt itself — only the client calls `grant`. That is exactly
 * what makes "explain before asking" something that can be said rather than something to arrange
 * afterwards.
 */

/** One row of the list, reduced for drawing. */
export interface DescribedOp {
  /** The operation kind — one of the 13 variants of `PrivilegedOp`. */
  kind: string;
  /** The sentence the daemon writes for the reader. Empty when there is none. */
  description: string;
  /** Exactly what it will change. Not translated: these are real paths, addresses and ports. */
  detail: string;
}

/** The `pending` of an `elevation_required`, or `null` when the message is about something else. */
export function pendingFrom(raw: string): unknown[] | null {
  try {
    const event = JSON.parse(raw) as { type?: unknown; pending?: unknown };
    if (event.type !== "elevation_required") return null;
    // An empty batch is still an answer: it means nothing is waiting any more.
    return Array.isArray(event.pending) ? event.pending : [];
  } catch {
    return null;
  }
}

/**
 * One `PendingOp` as three strings to draw.
 *
 * **`PendingOp` wraps `PrivilegedOp`; it is not one.** The real shape is
 * `{ id, op: PrivilegedOp, description, requested_at }` — so the operation kind sits at `op.op`,
 * one level deeper than the obvious guess. Reading the wrong level makes every row show `unknown`,
 * and the user is invited to allow a list that says nothing.
 *
 * `description` is the sentence the daemon writes. Prefer it: it comes from the side that knows
 * what the operation does, and rewriting it here would be MixDB making up a second explanation.
 */
export function describeOp(pending: unknown): DescribedOp {
  const row = (pending ?? {}) as { op?: unknown; description?: unknown };
  const description = typeof row.description === "string" ? row.description : "";

  const op = (row.op ?? {}) as {
    op?: unknown;
    entries?: unknown;
    plan?: unknown;
    target?: unknown;
  };
  const kind = typeof op.op === "string" ? op.op : "unknown";

  if (kind === "hosts-apply" && Array.isArray(op.entries)) {
    const lines = op.entries
      .map((entry) => {
        const line = entry as { address?: unknown; name?: unknown };
        return `${String(line.address ?? "")} ${String(line.name ?? "")}`.trim();
      })
      .filter(Boolean);
    return { kind, description, detail: lines.join("\n") };
  }

  // `TrustPlan.der` is a bare DER certificate — a few hundred numbers, one per line through
  // `JSON.stringify(..., null, 2)`. State only the size; do not dump the whole array.
  if (kind === "trust-ca-install") {
    const plan = op.plan as { method?: unknown; der?: unknown } | undefined;
    const method = typeof plan?.method === "string" ? plan.method : "";
    const bytes = Array.isArray(plan?.der) ? plan.der.length : 0;
    return { kind, description, detail: [method, `${bytes} bytes (DER)`].filter(Boolean).join("\n") };
  }

  // Every remaining variant shows its shape as is. An operation with no wording of its own must
  // still show up: hiding it asks for rights for something the user is not allowed to see.
  const extra = op.plan ?? op.target;
  return {
    kind,
    description,
    detail: extra === undefined ? "" : JSON.stringify(extra, null, 2),
  };
}
