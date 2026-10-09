import { describe, expect, it } from "vitest";

import { describeOp, isGrantInFlight, isNothingWaiting, otherGrant, pendingFrom } from "./pendingOps";

/** Exactly the shape the daemon sends: `PendingOp` wraps `PrivilegedOp` in the `op` field. */
const hostsApply = {
  id: 7,
  requested_at: 1788651901982,
  description: "Add 1 name to the hosts file",
  op: { op: "hosts-apply", entries: [{ address: "127.0.0.1", name: "blog.test" }] },
};

describe("pendingFrom", () => {
  it("takes the whole queue out of the event", () => {
    const raw = JSON.stringify({
      type: "elevation_required",
      pending: [hostsApply, { id: 8, op: { op: "trust-ca-install" }, description: "Trust the CA" }],
    });
    expect(pendingFrom(raw)).toHaveLength(2);
  });

  /* An empty batch is still an `elevation_required`: it means "nothing is waiting any more", quite
     different from "this event is not about administrator rights". */
  it("tells an empty queue from an event about something else", () => {
    expect(pendingFrom(JSON.stringify({ type: "elevation_required", pending: [] }))).toEqual([]);
    expect(pendingFrom(JSON.stringify({ type: "resync", missed: 1 }))).toBeNull();
    expect(pendingFrom("not json")).toBeNull();
  });
});

describe("describeOp", () => {
  /* The operation kind sits at `op.op`, one level deeper than the obvious guess. Reading the wrong
     level makes every row show `unknown`, and the user is invited to allow a list that says
     nothing. */
  it("reads the kind out of the op the pending entry wraps", () => {
    expect(describeOp(hostsApply).kind).toBe("hosts-apply");
  });

  /* The daemon's sentence comes from the side that knows what the operation does. Rewriting it in
     MixLab would make up a second explanation. */
  it("carries the daemon's own sentence", () => {
    expect(describeOp(hostsApply).description).toBe("Add 1 name to the hosts file");
  });

  it("names the hosts lines it would write", () => {
    const detail = describeOp(hostsApply).detail;
    expect(detail).toContain("blog.test");
    expect(detail).toContain("127.0.0.1");
  });

  /* 13 variants, and an unknown one must still show up — hiding it asks for rights for an operation
     the user is not allowed to see. */
  it("still describes an op it has no special wording for", () => {
    expect(describeOp({ id: 1, op: { op: "audit-log-remove" } }).kind).toBe("audit-log-remove");
    expect(describeOp({}).kind).toBe("unknown");
    expect(describeOp(null).kind).toBe("unknown");
    expect(describeOp(null).description).toBe("");
  });

  /* `TrustPlan.der` is a bare DER certificate — a few hundred numbers, one per line when dumped
     whole through `JSON.stringify`. The dialog must state the size, not dump the whole array onto
     the screen. */
  it("summarises a certificate's DER instead of dumping every byte", () => {
    const der = Array.from({ length: 402 }, (_, i) => i % 256);
    const described = describeOp({
      id: 3,
      op: { op: "trust-ca-install", plan: { method: "system-root", der } },
    });
    expect(described.detail).toBe("system-root\n402 bytes (DER)");
    expect(described.detail).not.toContain("[");
  });

  it("shows a plan or a target as it came", () => {
    const described = describeOp({ id: 2, op: { op: "port-access-grant", plan: { port: 443 } } });
    expect(described.detail).toContain("443");
  });

  /* Pins down the exact mistake that was made: a bare `PrivilegedOp` — the inner level — is not
     what travels on the wire, and reading it as if it were a `PendingOp` gives `unknown`. */
  it("does not mistake a bare privileged op for a pending entry", () => {
    expect(describeOp({ op: "hosts-apply", entries: [] }).kind).toBe("unknown");
  });
});

describe("isNothingWaiting", () => {
  /* The daemon raised the helper's prompt itself after an update, the person allowed it, and the
     dialog drawn from the same queue was still on screen: its "Allow" is answered with this. */
  it("recognises a grant refused because the queue was already empty", () => {
    expect(
      isNothingWaiting({
        code: "error.mixengineRefused",
        params: { code: "precondition_failed", message: "nothing is waiting for permission" },
      }),
    ).toBe(true);
  });

  it("leaves every other refusal to be shown", () => {
    expect(
      isNothingWaiting({
        code: "error.mixengineRefused",
        params: { code: "conflict", message: "a grant is already in flight" },
      }),
    ).toBe(false);
    expect(isNothingWaiting({ code: "error.mixengineUnreachable" })).toBe(false);
    expect(isNothingWaiting(new Error("boom"))).toBe(false);
    expect(isNothingWaiting(null)).toBe(false);
  });
});

describe("isGrantInFlight", () => {
  it("recognises the refusal of a second grant while one holds the prompt", () => {
    expect(
      isGrantInFlight({
        code: "error.mixengineRefused",
        params: { code: "conflict", message: "job 12 is already asking for permission" },
      }),
    ).toBe(true);
    expect(
      isGrantInFlight({
        code: "error.mixengineRefused",
        params: { code: "precondition_failed", message: "nothing is waiting for permission" },
      }),
    ).toBe(false);
  });
});

describe("otherGrant", () => {
  const waiting = [hostsApply];
  const declined = { job: 12, outcome: "declined" };

  it("keeps waiting while the last grant is still the one seen before", () => {
    expect(otherGrant(12, { pending: waiting, last: declined })).toEqual({ state: "waiting" });
    expect(otherGrant(null, { pending: waiting, last: null })).toEqual({ state: "waiting" });
  });

  it("hands back the grant that ended since", () => {
    expect(otherGrant(null, { pending: waiting, last: declined })).toEqual({
      state: "ended",
      grant: declined,
    });
    expect(otherGrant(11, { pending: [], last: declined })).toEqual({
      state: "ended",
      grant: declined,
    });
  });

  /* The grant applied everything and this read came before its outcome was recorded, or the queue
     was emptied some other way: either way there is nothing left to allow. */
  it("calls an empty queue done even with no new outcome", () => {
    expect(otherGrant(12, { pending: [], last: declined })).toEqual({ state: "emptied" });
  });
});
