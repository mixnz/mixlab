import { describe, expect, it } from "vitest";
import {
  call,
  type Capabilities,
  newRecord,
  opaqueId,
  put,
  remove,
  seed,
  signedUp,
  since,
} from "./client.js";

describe("tombstones", () => {
  /**
   * Three of the tests below need a tombstone that is still there, so none of them can run against
   * a server configured to reap at once — it may sweep between the delete and the assertion, and
   * a record whose tombstone has legitimately gone is legitimately absent. Skipping is the honest
   * answer: the alternative is a test that passes on whichever side of a one-second race it lands.
   */
  const needsASurvivingTombstone = async (skip: (note: string) => void) => {
    const limits = (await call<Capabilities>("/v1/capabilities")).body;
    if (limits.tombstoneRetentionDays === 0) {
      skip("this server reaps tombstones at once, so none of them outlives the request that made it");
    }
  };

  it("travel on the cursor so another machine learns of the deletion", async ({ skip }) => {
    await needsASurvivingTombstone(skip);
    const { session } = await signedUp();
    const { collection, id, stored } = await seed(session.accessToken);
    const deleted = await remove(session.accessToken, collection, id, stored.version);

    const page = await since(session.accessToken, stored.seq, collection);
    const tombstone = page.body.records.find((record) => record.id === id);

    expect(tombstone?.deleted).toBe(true);
    expect(tombstone?.seq).toBe(deleted.body.seq);
    expect(tombstone?.ciphertext ?? null).toBeNull();
  });

  it("can be brought back to life by a write", async ({ skip }) => {
    await needsASurvivingTombstone(skip);
    // A person who deletes a saved query on one machine and creates one with the same local uuid
    // on another must not be told the record does not exist. The tombstone is a version like any
    // other, and a matching If-Match writes over it.
    const { session } = await signedUp();
    const { collection, id, stored } = await seed(session.accessToken);
    const tombstone = await remove(session.accessToken, collection, id, stored.version);

    const revived = await put(session.accessToken, collection, id, newRecord(), {
      ifMatch: tombstone.body.version,
    });
    expect(revived.status).toBe(200);
    expect(revived.body.deleted).toBe(false);
    expect(revived.body.version).toBe(tombstone.body.version + 1);
    expect(revived.body.ciphertext).toBeTypeOf("string");
  });

  it("do not let a creation slip past a deletion", async ({ skip }) => {
    await needsASurvivingTombstone(skip);
    const { session } = await signedUp();
    const { collection, id, stored } = await seed(session.accessToken);
    await remove(session.accessToken, collection, id, stored.version);

    const created = await put(session.accessToken, collection, id, newRecord(), { ifNoneMatch: true });
    expect(created.status).toBe(412);
    expect(created.body.error?.code).toBe("already-exists");
  });

  it("expire a cursor that has been away longer than they live", async ({ skip }) => {
    // D3: a machine offline longer than the retention is told to resync from empty rather than
    // told incomplete news quietly. The suite can only reach that state against a server
    // configured to reap at once; ninety days is not something a test can wait out.
    const limits = (await call<Capabilities>("/v1/capabilities")).body;
    if (limits.tombstoneRetentionDays !== 0) {
      skip(
        `this server reports tombstoneRetentionDays=${limits.tombstoneRetentionDays}. Run a second ` +
          "instance with a retention of 0 to cover cursor expiry; no test can wait ninety days.",
      );
    }

    const { session } = await signedUp();
    // A cursor that has seen something, and a deletion after it. `since=0` is exempt on purpose:
    // a machine with no history has missed nothing, so it is told to start rather than to resync.
    const anchor = await seed(session.accessToken);
    const doomed = await seed(session.accessToken);
    await remove(session.accessToken, doomed.collection, doomed.id, doomed.stored.version);

    // Reaping is a scheduled job and not part of the write that caused it — D8 schedules an alarm
    // rather than sweeping inline, so a test has to allow for the gap rather than assume it away.
    let page = await since(session.accessToken, anchor.stored.seq);
    for (let attempt = 0; attempt < 40 && page.status !== 410; attempt += 1) {
      await new Promise((resume) => setTimeout(resume, 500));
      page = await since(session.accessToken, anchor.stored.seq);
    }

    expect(page.status).toBe(410);
    expect(page.body.error?.code).toBe("cursor-expired");
  });

  it("do not expire the cursor a resync of one collection has just handed out", async ({ skip }) => {
    // A client told 410 pulls its collection again from 0 and keeps the `nextSince` it is given.
    // That cursor has missed nothing; refused on the next pull because a tombstone reaped in some
    // other collection sits above it, the client starts over on every pull, for ever.
    const limits = (await call<Capabilities>("/v1/capabilities")).body;
    if (limits.tombstoneRetentionDays !== 0) {
      skip(
        `this server reports tombstoneRetentionDays=${limits.tombstoneRetentionDays}. Run a second ` +
          "instance with a retention of 0 to cover cursor expiry; no test can wait ninety days.",
      );
    }

    const { session } = await signedUp();
    // `seed` puts each record in a collection of its own: `quiet` is never written again.
    const quiet = await seed(session.accessToken);
    const doomed = await seed(session.accessToken);
    await remove(session.accessToken, doomed.collection, doomed.id, doomed.stored.version);

    let page = await since(session.accessToken, quiet.stored.seq, quiet.collection);
    for (let attempt = 0; attempt < 40 && page.status !== 410; attempt += 1) {
      await new Promise((resume) => setTimeout(resume, 500));
      page = await since(session.accessToken, quiet.stored.seq, quiet.collection);
    }
    expect(page.status, "the story needs the tombstone reaped").toBe(410);

    const resync = await since(session.accessToken, 0, quiet.collection);
    expect(resync.status).toBe(200);
    const next = await since(session.accessToken, resync.body.nextSince, quiet.collection);
    expect(next.status, `nextSince=${resync.body.nextSince} was expired as soon as it was handed out`).toBe(200);
  });

  it("let a read from 0 page to its end with resync=1", async ({ skip }) => {
    // Past one page, a resync's cursor is the first page's last seq, which can sit below the reap
    // mark: expired again, the client starts over for ever. `resync=1` says the cursor came from a
    // read that began at 0, which has missed nothing.
    const limits = (await call<Capabilities>("/v1/capabilities")).body;
    if (limits.tombstoneRetentionDays !== 0) {
      skip(
        `this server reports tombstoneRetentionDays=${limits.tombstoneRetentionDays}. Run a second ` +
          "instance with a retention of 0 to cover cursor expiry; no test can wait ninety days.",
      );
    }
    if (limits.maxPageRecords > 50) {
      skip(`maxPageRecords=${limits.maxPageRecords}: more records than a test should write to fill two pages`);
    }

    const { session } = await signedUp();
    const collection = opaqueId();
    const count = limits.maxPageRecords + 1;
    for (let n = 0; n < count; n += 1) {
      await put(session.accessToken, collection, opaqueId(), newRecord(), { ifNoneMatch: true });
    }
    const doomed = await seed(session.accessToken);
    await remove(session.accessToken, doomed.collection, doomed.id, doomed.stored.version);

    let probe = await since(session.accessToken, 1, collection);
    for (let attempt = 0; attempt < 40 && probe.status !== 410; attempt += 1) {
      await new Promise((resume) => setTimeout(resume, 500));
      probe = await since(session.accessToken, 1, collection);
    }
    expect(probe.status, "the story needs the tombstone reaped").toBe(410);

    const first = await since(session.accessToken, 0, collection);
    expect(first.body.more).toBe(true);
    const without = await since(session.accessToken, first.body.nextSince, collection);
    expect(without.status, "without the flag the second page is expired, which is the fault").toBe(410);

    let seen = first.body.records.length;
    let page = first;
    while (page.body.more) {
      page = await since(session.accessToken, page.body.nextSince, collection, { resync: true });
      expect(page.status).toBe(200);
      seen += page.body.records.length;
    }
    expect(seen).toBe(count);
  });

  it("do not expire a cursor that is current", async () => {
    const { session } = await signedUp();
    const page = await since(session.accessToken, 0);
    expect(page.status).toBe(200);
  });
});
