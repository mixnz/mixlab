import { describe, expect, it } from "vitest";

import {
  describeOp,
  hasNewOps,
  isGrantInFlight,
  isNothingWaiting,
  otherGrant,
  pendingFrom,
  terminalCommand,
  whatTheGrantWaitsFor,
} from "./pendingOps";

/** Exactly the shape the daemon sends: `PendingOp` wraps `PrivilegedOp` in the `op` field. */
const hostsApply = {
  id: 7,
  requested_at: 1788651901982,
  description: "Add 1 name to the hosts file",
  op: {
    op: "hosts-apply",
    home: "0123456789abcdef",
    entries: [
      { address: "127.0.0.1", domain: "blog.test" },
      { address: "127.0.0.1", domain: "shop.test" },
    ],
  },
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

  /* T211h: each line is an address and a domain, as `HostEntry` spells them. Reading a `name` the
     binding does not have printed `127.0.0.1` once per line and no name at all. */
  it("names the hosts lines it would write", () => {
    const detail = describeOp(hostsApply).detail;
    expect(detail).toBe("127.0.0.1 blog.test\n127.0.0.1 shop.test");
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

describe("hasNewOps", () => {
  const trust = { id: 8, op: { op: "trust-ca-install" }, description: "Trust the CA" };

  it("sees a queue that grew", () => {
    expect(hasNewOps(new Set(), [hostsApply])).toBe(true);
    expect(hasNewOps(new Set([7]), [hostsApply, trust])).toBe(true);
  });

  /* A grant that applied one row and kept a failed one announces the row it kept. The dialog that
     asked has just closed; opening it again for that row is not something anybody asked for. */
  it("does not count a queue that shrank, or stayed the same", () => {
    expect(hasNewOps(new Set([7, 8]), [hostsApply])).toBe(false);
    expect(hasNewOps(new Set([7]), [hostsApply])).toBe(false);
    expect(hasNewOps(new Set([7]), [])).toBe(false);
  });
});

describe("terminalCommand", () => {
  /* ADR 0065: a grant `mix` started over SSH waits for this line; the window shows it so that it can
     be run, and never runs it — MixLab has no terminal. */
  it("is sudo on the helper and the request the daemon is waiting on", () => {
    expect(
      terminalCommand({
        job: 4,
        helper: "/usr/local/libexec/mixengine/mixengine-elevate",
        request: "/home/me/.mixengine/run/elevate/abc/request.json",
      }),
    ).toBe(
      "sudo -- /usr/local/libexec/mixengine/mixengine-elevate /home/me/.mixengine/run/elevate/abc/request.json",
    );
  });

  it("is nothing when no grant is waiting", () => {
    expect(terminalCommand(null)).toBeNull();
    expect(terminalCommand(undefined)).toBeNull();
  });
});

describe("describeOp's screenOnly", () => {
  /* The daemon's sentence for a row a terminal cannot grant: on a Mac, adding the authority to the
     System keychain asks on the Mac's own screen. */
  it("carries the daemon's sentence", () => {
    const row = { ...hostsApply, needs_the_screen: "it needs this Mac's own screen" };
    expect(describeOp(row).screenOnly).toBe("it needs this Mac's own screen");
  });

  it("is empty for a row a terminal can grant", () => {
    expect(describeOp(hostsApply).screenOnly).toBe("");
  });
});

describe("whatTheGrantWaitsFor", () => {
  /* Following a grant a terminal started, there is no system prompt anywhere: saying there is one
     sent a person looking for a dialog that did not exist. */
  it("is the terminal while a terminal's command is waiting", () => {
    expect(whatTheGrantWaitsFor("sudo -- /x /y")).toBe("terminal");
  });

  it("is the system prompt otherwise", () => {
    expect(whatTheGrantWaitsFor(null)).toBe("systemPrompt");
  });
});
