import { describe, expect, it } from "vitest";
import {
  call,
  type Capabilities,
  heads,
  login,
  newRecord,
  opaqueId,
  put,
  remove,
  seed,
  signedUp,
  since,
  type StoredRecord,
} from "./client.js";

/** Every page of `collection` after `from`, or `expired` when the first answer is `410`. */
async function pulled(token: string, collection: string, from: number) {
  let page = await since(token, from, collection);
  if (page.status === 410) return { expired: true, records: [] as StoredRecord[], nextSince: from };
  const records = [...page.body.records];
  while (page.body.more) {
    page = await since(token, page.body.nextSince, collection);
    records.push(...page.body.records);
  }
  return { expired: false, records, nextSince: page.body.nextSince };
}

describe("heads", () => {
  it("names a collection another machine wrote, and not to the machine that wrote it", async () => {
    const { account, session: a } = await signedUp("a");
    const b = await login(account, "b");
    const collection = opaqueId();
    const first = await put(a.accessToken, collection, opaqueId(), newRecord(), { ifNoneMatch: true });
    await put(a.accessToken, collection, opaqueId(), newRecord(), { ifNoneMatch: true });

    const mine = await heads(a.accessToken, { [collection]: first.body.seq });
    const theirs = await heads(b.accessToken, { [collection]: first.body.seq });

    expect(mine.status).toBe(200);
    expect(mine.body.stale).toEqual([]);
    expect(theirs.body.stale).toEqual([collection]);
  });

  it("names a collection another machine deleted from", async () => {
    const { account, session: a } = await signedUp("a");
    const b = await login(account, "b");
    const { collection, id, stored } = await seed(a.accessToken);
    await remove(b.accessToken, collection, id, stored.version);

    const answer = await heads(a.accessToken, { [collection]: stored.seq });
    expect(answer.body.stale).toEqual([collection]);
  });

  it("ends at the seq a pull read to its last page ends at", async () => {
    const { session } = await signedUp();
    const { collection, stored } = await seed(session.accessToken);
    // Another collection moves the account's seq past this one's last row (M3).
    await seed(session.accessToken);

    const answer = await heads(session.accessToken, { [collection]: stored.seq });
    const pull = await pulled(session.accessToken, collection, stored.seq);
    expect(answer.body.nextSince).toBe(pull.nextSince);
  });

  it("calls a cursor of 0 stale when the collection holds anything, even this device's own", async () => {
    const { session } = await signedUp();
    const { collection } = await seed(session.accessToken);
    const empty = opaqueId();

    const answer = await heads(session.accessToken, { [collection]: 0, [empty]: 0 });
    expect(answer.body.stale).toEqual([collection]);
  });

  it("agrees with the pull, cursor by cursor, for both devices", async ({ skip }) => {
    // Against a server that reaps at once, the sweep may land between the two questions of one
    // cursor, and the test would pass or fail on which side of that race it fell.
    const limits = (await call<Capabilities>("/v1/capabilities")).body;
    if (limits.tombstoneRetentionDays === 0) {
      skip("this server reaps tombstones in the background at once; the expiry test covers it");
    }
    const { account, session: a } = await signedUp("a");
    const b = await login(account, "b");
    const collection = opaqueId();
    const one = await put(a.accessToken, collection, opaqueId(), newRecord(), { ifNoneMatch: true });
    await put(b.accessToken, collection, opaqueId(), newRecord(), { ifNoneMatch: true });
    await put(a.accessToken, collection, opaqueId(), newRecord(), { ifNoneMatch: true });
    const gone = await remove(a.accessToken, collection, one.body.id, one.body.version);

    for (const device of [a, b]) {
      for (let cursor = 0; cursor <= gone.body.seq; cursor += 1) {
        const answer = await heads(device.accessToken, { [collection]: cursor });
        const pull = await pulled(device.accessToken, collection, cursor);
        const news =
          pull.expired || pull.records.some((record) => cursor === 0 || record.device !== device.deviceId);
        expect(answer.body.stale.includes(collection), `device ${device.deviceId}, cursor ${cursor}`).toBe(news);
      }
    }
  });

  it("calls an expired cursor stale, where the pull answers 410", async ({ skip }) => {
    const limits = (await call<Capabilities>("/v1/capabilities")).body;
    if (limits.tombstoneRetentionDays !== 0) {
      skip(
        `this server reports tombstoneRetentionDays=${limits.tombstoneRetentionDays}. Run a second ` +
          "instance with a retention of 0 to cover cursor expiry; no test can wait ninety days.",
      );
    }
    const { session } = await signedUp();
    // `quiet` is never written again: only the reap mark can make it stale.
    const quiet = await seed(session.accessToken);
    const doomed = await seed(session.accessToken);
    await remove(session.accessToken, doomed.collection, doomed.id, doomed.stored.version);

    // Reaping is a scheduled job (D8), so the test waits for it rather than assuming it.
    let answer = await heads(session.accessToken, { [quiet.collection]: quiet.stored.seq });
    for (let attempt = 0; attempt < 40 && answer.body.stale.length === 0; attempt += 1) {
      await new Promise((resume) => setTimeout(resume, 500));
      answer = await heads(session.accessToken, { [quiet.collection]: quiet.stored.seq });
    }
    expect(answer.body.stale).toEqual([quiet.collection]);
    expect((await since(session.accessToken, quiet.stored.seq, quiet.collection)).status).toBe(410);
  });

  it("refuses what is not a list of cursors", async () => {
    const { session } = await signedUp();
    const limits = (await call<Capabilities>("/v1/capabilities")).body;
    const tooMany = Object.fromEntries(
      Array.from({ length: limits.maxBatchOperations + 1 }, () => [opaqueId(), 0]),
    );
    const collection = opaqueId();
    for (const cursors of [
      {},
      tooMany,
      [collection],
      null,
      { "not-hex": 0 },
      { [collection]: -1 },
      { [collection]: 1.5 },
      { [collection]: "5" },
    ]) {
      const answer = await heads(session.accessToken, cursors);
      expect(answer.status, JSON.stringify(cursors).slice(0, 80)).toBe(400);
      expect(answer.body.error?.code).toBe("invalid-request");
    }
  });

  it("answers a frozen account, since it only reads", async () => {
    const { session } = await signedUp();
    const { collection, stored } = await seed(session.accessToken);
    const frozen = await call("/v1/account/freeze", { body: { state: "frozen" }, token: session.accessToken });
    expect(frozen.status).toBe(200);

    const answer = await heads(session.accessToken, { [collection]: stored.seq });
    expect(answer.status).toBe(200);
  });

  it("needs a session", async () => {
    const answer = await heads("", { [opaqueId()]: 0 });
    expect(answer.status).toBe(401);
  });
});
