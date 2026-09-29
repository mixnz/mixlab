import { describe, expect, it } from "vitest";

import { eventArrived, noReadsYet, readBegan, readLanded } from "./readOrder";

describe("readLanded", () => {
  /* Exactly the "Stop all, then a service stuck at Stopping" bug: two `reload()`s run in parallel,
     the one that set off first reads the old state, and if its response comes back later it
     overwrites the newer snapshot. The table stays at `stopping` until someone presses Refresh. */
  it("refuses a snapshot that started before one already applied", () => {
    const first = readBegan(noReadsYet());
    const second = readBegan(first.order);

    const newer = readLanded(second.order, second.seq);
    expect(newer.apply).toBe(true);

    const older = readLanded(newer.order, first.seq);
    expect(older.apply).toBe(false);
  });
});

describe("eventArrived", () => {
  /* An event arriving while `service.list` is on its way back carries news from *before* the daemon
     read the list — the client has no way to tell it from newer news. So the snapshot that just
     landed can no longer be trusted on its own: one more read is needed. Without this step a late
     `stopping` overwrites `stopped`, and since `stopped` is the final transition, no further event
     corrects it. */
  it("asks for one more read when an event landed mid-flight", () => {
    const started = readBegan(noReadsYet());
    const raced = eventArrived(started.order);

    const landed = readLanded(raced, started.seq);
    expect(landed.apply).toBe(true);
    expect(landed.readAgain).toBe(true);
  });

  /* An event while no `reload` is in flight has nothing to race: the row has already changed with
     the stream, and that is the right answer. Rereading here would turn every Start click into an
     extra round of RPCs. */
  it("does not ask for a read when no read was in flight", () => {
    const quiet = eventArrived(noReadsYet());
    const started = readBegan(quiet);
    expect(readLanded(started.order, started.seq).readAgain).toBe(false);
  });

  /* Two `reload`s in flight together (coming back to the tab just as a `resync` arrives) and an
     event slipping in between: only **one** extra read is requested, when the last one lands.
     Requesting one per landing turns one event into N rounds of RPCs, and each of those is another
     window for the next event to slip into. */
  it("asks for exactly one more read however many were in flight", () => {
    const first = readBegan(noReadsYet());
    const second = readBegan(first.order);
    const raced = eventArrived(second.order);

    const firstLanded = readLanded(raced, first.seq);
    expect(firstLanded.readAgain).toBe(false);

    const secondLanded = readLanded(firstLanded.order, second.seq);
    expect(secondLanded.readAgain).toBe(true);

    /* And that extra read, if no further event arrives, does not produce a third. */
    const again = readBegan(secondLanded.order);
    expect(readLanded(again.order, again.seq).readAgain).toBe(false);
  });
});
