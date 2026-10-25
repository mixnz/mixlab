import { beforeAll, describe, expect, it } from "vitest";
import {
  base64Bytes,
  call,
  type Capabilities,
  type ErrorBody,
  type Page,
  newRecord,
  opaqueId,
  put,
  remove,
  seed,
  type Session,
  signedUp,
  since,
} from "./client.js";

let session: Session;
let limits: Capabilities;

beforeAll(async () => {
  session = (await signedUp()).session;
  limits = (await call<Capabilities>("/v1/capabilities")).body;
});

describe("creating a record", () => {
  it("assigns version 1 and a sequence number, and echoes what it was given", async () => {
    const collection = opaqueId();
    const id = opaqueId();
    const body = newRecord();
    const result = await put(session.accessToken, collection, id, body, { ifNoneMatch: true });

    expect(result.status).toBe(201);
    expect(result.body.version).toBe(1);
    expect(result.body.seq).toBeGreaterThan(0);
    expect(result.body.collection).toBe(collection);
    expect(result.body.id).toBe(id);
    expect(result.body.deleted).toBe(false);
    expect(result.body.nonce).toBe(body.nonce);
    expect(result.body.ciphertext).toBe(body.ciphertext);
    expect(result.body.updatedAt).toBe(body.updatedAt);
    expect(result.headers.get("etag")).toBe('"1"');
  });

  it("refuses to create one twice, and hands back what is there", async () => {
    const { collection, id } = await seed(session.accessToken);
    const again = await put(session.accessToken, collection, id, newRecord(), { ifNoneMatch: true });

    expect(again.status).toBe(412);
    expect(again.body.error?.code).toBe("already-exists");
    expect(again.body.version).toBe(1);
  });
});

describe("updating a record", () => {
  it("moves the version on when If-Match agrees", async () => {
    const { collection, id, stored } = await seed(session.accessToken);
    const updated = await put(session.accessToken, collection, id, newRecord(), {
      ifMatch: stored.version,
    });

    expect(updated.status).toBe(200);
    expect(updated.body.version).toBe(stored.version + 1);
    expect(updated.body.seq).toBeGreaterThan(stored.seq);
    expect(updated.headers.get("etag")).toBe('"2"');
  });

  it("refuses a lost update and hands back the record that won", async () => {
    const { collection, id, stored } = await seed(session.accessToken);
    await put(session.accessToken, collection, id, newRecord(), { ifMatch: stored.version });

    const stale = await put(session.accessToken, collection, id, newRecord(), {
      ifMatch: stored.version,
    });
    expect(stale.status).toBe(409);
    expect(stale.body.error?.code).toBe("version-conflict");
    // The client resolves the conflict, so it needs the other side of it in the same answer (D4).
    expect(stale.body.version).toBe(2);
    expect(stale.body.ciphertext).toBeTypeOf("string");
  });

  it("refuses a write that states no precondition at all", async () => {
    const { collection, id } = await seed(session.accessToken);
    const result = await put(session.accessToken, collection, id, newRecord(), {});

    expect(result.status).toBe(428);
    expect(result.body.error?.code).toBe("precondition-required");
  });

  it("never compares the client's clock", async () => {
    // D1 promises the server compares nothing. A machine whose clock runs behind still writes.
    const { collection, id, stored } = await seed(session.accessToken);
    const backwards = { ...newRecord(), updatedAt: stored.updatedAt - 86_400 };
    const result = await put(session.accessToken, collection, id, backwards, {
      ifMatch: stored.version,
    });

    expect(result.status).toBe(200);
    expect(result.body.updatedAt).toBe(backwards.updatedAt);
  });
});

describe("deleting a record", () => {
  it("writes a tombstone that carries no ciphertext", async () => {
    const { collection, id, stored } = await seed(session.accessToken);
    const deleted = await remove(session.accessToken, collection, id, stored.version);

    expect(deleted.status).toBe(200);
    expect(deleted.body.deleted).toBe(true);
    expect(deleted.body.version).toBe(stored.version + 1);
    expect(deleted.body.ciphertext ?? null).toBeNull();
  });

  it("does not move a tombstone that is already there", async () => {
    // Without this, a delete retried after a dropped connection bumps `seq`, and every other
    // machine pulls a change that is not one (D4a).
    const { collection, id, stored } = await seed(session.accessToken);
    const first = await remove(session.accessToken, collection, id, stored.version);
    const again = await remove(session.accessToken, collection, id, first.body.version);

    expect(again.status).toBe(200);
    expect(again.body.version).toBe(first.body.version);
    expect(again.body.seq).toBe(first.body.seq);
  });

  it("keeps the time the deletion was made", async () => {
    // A tombstone is an edit with a time: D4 weighs it against an edit made elsewhere, and a
    // tombstone that kept the replaced version's time would lose to any later edit, or win, by
    // arrival order alone.
    const { collection, id, stored } = await seed(session.accessToken);
    const deleted = await remove(session.accessToken, collection, id, stored.version, 1_900_000_000);
    expect(deleted.status).toBe(200);
    expect(deleted.body.updatedAt).toBe(1_900_000_000);
  });

  it("refuses a deletion that does not say when it was made", async () => {
    const { collection, id, stored } = await seed(session.accessToken);
    const result = await remove(session.accessToken, collection, id, stored.version, null);
    expect(result.status).toBe(400);
    expect(result.body.error?.code).toBe("invalid-request");
  });

  it("refuses to delete what was never there", async () => {
    const result = await remove(session.accessToken, opaqueId(), opaqueId(), 1);
    expect(result.status).toBe(404);
    expect(result.body.error?.code).toBe("unknown-record");
  });

  it("refuses a delete with a stale If-Match", async () => {
    const { collection, id, stored } = await seed(session.accessToken);
    await put(session.accessToken, collection, id, newRecord(), { ifMatch: stored.version });

    const stale = await remove(session.accessToken, collection, id, stored.version);
    expect(stale.status).toBe(409);
    expect(stale.body.error?.code).toBe("version-conflict");
  });
});

describe("what a record may be", () => {
  it.each([
    ["too short", "abc"],
    ["not hex", "z".repeat(64)],
    ["uppercase", "A".repeat(64)],
  ])("refuses an address that is %s", async (_label, bad) => {
    const result = await put(session.accessToken, bad, opaqueId(), newRecord(), { ifNoneMatch: true });
    expect(result.status).toBe(400);
    expect(result.body.error?.code).toBe("invalid-request");
  });

  it("refuses a record larger than it said it would take", async () => {
    // base64 carries three bytes in four characters, so this overshoots the limit whatever the
    // server counts — the encoded length or the decoded one.
    const oversized = {
      ...newRecord(),
      ciphertext: base64Bytes(limits.maxRecordBytes + 1024),
    };
    const result = await put(session.accessToken, opaqueId(), opaqueId(), oversized, {
      ifNoneMatch: true,
    });

    expect(result.status).toBe(413);
    expect(result.body.error?.code).toBe("record-too-large");
  });
});

describe("GET /v1/records", () => {
  it("ends the last page of a collection at the account's latest seq", async () => {
    // A cursor stopped at a quiet collection's own last row sits below every later write in the
    // account, where one reaped tombstone expires it for good.
    const { session: own } = await signedUp();
    const quiet = await seed(own.accessToken);
    const later = await seed(own.accessToken);
    const page = await since(own.accessToken, 0, quiet.collection);
    expect(page.status).toBe(200);
    expect(page.body.more).toBe(false);
    expect(page.body.nextSince).toBe(later.stored.seq);
  });

  it("refuses a resync flag that is not 1", async () => {
    const { session: own } = await signedUp();
    for (const bad of ["true", "0", ""]) {
      const result = await call<ErrorBody>(`/v1/records?since=0&resync=${encodeURIComponent(bad)}`, {
        token: own.accessToken,
      });
      expect(result.status, `resync=${JSON.stringify(bad)}`).toBe(400);
      expect(result.body.error?.code).toBe("invalid-request");
    }
  });

  it("returns what changed after the cursor, oldest first", async () => {
    const { session: own } = await signedUp();
    const collection = opaqueId();
    const ids = [opaqueId(), opaqueId(), opaqueId()];
    for (const id of ids) {
      await put(own.accessToken, collection, id, newRecord(), { ifNoneMatch: true });
    }

    const page = await since(own.accessToken, 0, collection);
    expect(page.status).toBe(200);
    expect(page.body.records.map((record) => record.id)).toEqual(ids);
    expect(page.body.records.map((record) => record.seq)).toEqual(
      [...page.body.records.map((record) => record.seq)].sort((a, b) => a - b),
    );
    expect(page.body.nextSince).toBe(page.body.records.at(-1)?.seq);
  });

  it("refuses a cursor that is not a number", async () => {
    // `Number("")` is 0 in JavaScript, so `?since=` quietly meant "from the beginning" on one
    // implementation and was refused by the other. Neither is wrong on its own; disagreeing is.
    const { session: own } = await signedUp();
    for (const bad of ["", "abc", "-1", "1.5", " 1"]) {
      const result = await call<Page & Partial<ErrorBody>>(
        `/v1/records?since=${encodeURIComponent(bad)}`,
        { token: own.accessToken },
      );
      expect(result.status, `since=${JSON.stringify(bad)}`).toBe(400);
      expect(result.body.error?.code).toBe("invalid-request");
    }
  });

  it("treats the cursor as exclusive", async () => {
    const { session: own } = await signedUp();
    const collection = opaqueId();
    const first = await put(own.accessToken, collection, opaqueId(), newRecord(), {
      ifNoneMatch: true,
    });
    await put(own.accessToken, collection, opaqueId(), newRecord(), { ifNoneMatch: true });

    const page = await since(own.accessToken, first.body.seq, collection);
    expect(page.body.records.map((record) => record.seq)).not.toContain(first.body.seq);
    expect(page.body.records).toHaveLength(1);
  });

  it("returns every collection when none is named", async () => {
    // The broad form is what a burst sync wants: what costs an account is how often a client
    // wakes the server, not how much one answer carries (D8).
    const { session: own } = await signedUp();
    const left = opaqueId();
    const right = opaqueId();
    await put(own.accessToken, left, opaqueId(), newRecord(), { ifNoneMatch: true });
    await put(own.accessToken, right, opaqueId(), newRecord(), { ifNoneMatch: true });

    const page = await since(own.accessToken, 0);
    expect(new Set(page.body.records.map((record) => record.collection))).toEqual(new Set([left, right]));
  });

  it("narrows to one collection when one is named", async () => {
    const { session: own } = await signedUp();
    const wanted = opaqueId();
    await put(own.accessToken, wanted, opaqueId(), newRecord(), { ifNoneMatch: true });
    await put(own.accessToken, opaqueId(), opaqueId(), newRecord(), { ifNoneMatch: true });

    const page = await since(own.accessToken, 0, wanted);
    expect(page.body.records).toHaveLength(1);
    expect(page.body.records[0]?.collection).toBe(wanted);
  });

  it("shows another account nothing", async () => {
    const mine = await signedUp();
    const theirs = await signedUp();
    const { collection } = await seed(mine.session.accessToken);

    const page = await since(theirs.session.accessToken, 0, collection);
    expect(page.body.records).toHaveLength(0);
  });

  it("stops at the page size it reported, and says there is more", async ({ skip }) => {
    if (limits.maxPageRecords > 200) {
      skip(
        `this server reports maxPageRecords=${limits.maxPageRecords}; filling a page would take ` +
          "minutes. Configure a smaller page on the instance under test to cover this.",
      );
    }
    const { session: own } = await signedUp();
    const collection = opaqueId();
    const wanted = limits.maxPageRecords + 1;
    for (let written = 0; written < wanted; written += limits.maxBatchOperations) {
      const operations = Array.from(
        { length: Math.min(limits.maxBatchOperations, wanted - written) },
        () => ({ op: "put", collection, id: opaqueId(), ifNoneMatch: true, record: newRecord(16) }),
      );
      await call("/v1/records/batch", { body: { operations }, token: own.accessToken });
    }

    const page = await since(own.accessToken, 0, collection);
    expect(page.body.records).toHaveLength(limits.maxPageRecords);
    expect(page.body.more).toBe(true);

    const rest = await since(own.accessToken, page.body.nextSince, collection);
    expect(rest.body.records).toHaveLength(1);
    expect(rest.body.more).toBe(false);
  });
});
