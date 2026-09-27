import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SyncChanges, SyncableCollection } from "../../core/syncCollection";
import type { PulledPage, PushedChanges, SyncBackend } from "./api";
import {
  FOCUS_COALESCE_MS,
  FOCUS_PULL_MS,
  IDLE_PULL_MS,
  LOCAL_CHECK_MS,
  pushCollection,
  startSyncLoop,
  syncCollection,
  type RunResult,
} from "./loop";

const nothing: SyncChanges = { upserts: [], removed: [] };
const one: SyncChanges = { upserts: [{ id: "a", data: 1 }], removed: [] };

/** A backend that says what it was asked, in order. `stale` is what `heads` names. */
function backend(
  pages: PulledPage[],
  pushed: PushedChanges = { accepted: 0, replaced: nothing, token: null, error: null, needsPull: false },
  stale: (ids: string[]) => Promise<string[]> = async (ids) => ids,
) {
  const calls: string[] = [];
  const pulled: string[] = [];
  const queue = [...pages];
  const fake: SyncBackend = {
    notice: async () => void calls.push("notice"),
    heads: async (ids) => {
      calls.push("heads");
      return stale(ids);
    },
    pullPage: async (collection) => {
      calls.push("pull");
      pulled.push(collection);
      return queue.shift() ?? { token: "end", changes: nothing, more: false };
    },
    commitPull: async (_c, token) => void calls.push(`commitPull ${token}`),
    push: async () => {
      calls.push("push");
      return pushed;
    },
    commitPush: async (_c, token) => void calls.push(`commitPush ${token}`),
  };
  return { fake, calls, pulled };
}

function collection(
  calls: string[],
  write: (changes: SyncChanges) => Promise<void> = async () => {},
  id = "c",
): SyncableCollection {
  return {
    id,
    labelKey: "sync.preferences",
    read: async () => {
      calls.push("read");
      return [];
    },
    write: async (changes) => {
      calls.push("write");
      await write(changes);
      return [];
    },
  };
}

describe("one collection", () => {
  it("writes each page before committing it, and pushes after the last", async () => {
    const { fake, calls } = backend([
      { token: "p1", changes: one, more: true },
      { token: "p2", changes: one, more: false },
    ]);
    await syncCollection(fake, collection(calls));
    expect(calls).toEqual([
      "read",
      "notice",
      "pull",
      "write",
      "commitPull p1",
      "pull",
      "write",
      "commitPull p2",
      "read",
      "push",
    ]);
  });

  it("does not commit a page its module could not write, and does not push", async () => {
    const { fake, calls } = backend([{ token: "p1", changes: one, more: false }]);
    const failing = collection(calls, async () => {
      throw new Error("disk full");
    });
    await expect(syncCollection(fake, failing)).rejects.toThrow("disk full");
    expect(calls).toEqual(["read", "notice", "pull", "write"]);
  });

  it("moves past an empty page without asking the module to write nothing", async () => {
    const { fake, calls } = backend([{ token: "p1", changes: nothing, more: false }]);
    await syncCollection(fake, collection(calls));
    expect(calls).toEqual(["read", "notice", "pull", "commitPull p1", "read", "push"]);
  });

  it("writes a lost conflict's winner, commits it, and counts it", async () => {
    const { fake, calls } = backend([], { accepted: 0, replaced: one, token: "w1", error: null, needsPull: false });
    expect(await pushCollection(fake, collection(calls))).toBe(1);
    expect(calls).toEqual(["read", "push", "write", "commitPush w1"]);
  });

  it("leaves a winner uncommitted when writing it fails, so the next push meets it again", async () => {
    const { fake, calls } = backend([], { accepted: 0, replaced: one, token: "w1", error: null, needsPull: false });
    const failing = collection(calls, async () => {
      throw new Error("disk full");
    });
    await expect(pushCollection(fake, failing)).rejects.toThrow("disk full");
    expect(calls).not.toContain("commitPush w1");
  });

  it("writes and commits a lost conflict's winner before it reports a refused entry", async () => {
    // What landed and what was replaced are recorded first; the refusal is reported after, the
    // way a failed push always was (T178c, C2).
    const refusal = { code: "error.syncRecordTooLarge" };
    const { fake, calls } = backend([], { accepted: 1, replaced: one, token: "w1", error: refusal, needsPull: false });
    await expect(pushCollection(fake, collection(calls))).rejects.toEqual(refusal);
    expect(calls).toEqual(["read", "push", "write", "commitPush w1"]);
  });

  it("pulls a collection first when this account never has, then pushes", async () => {
    // A push trusts that what this machine never saw the server does not have. After a move, the
    // new account's store was empty and a push rewrote every record the move had just copied.
    const calls: string[] = [];
    let pushes = 0;
    const fake: SyncBackend = {
      notice: async () => void calls.push("notice"),
      heads: async (ids) => ids,
      pullPage: async () => {
        calls.push("pull");
        return { token: "p1", changes: nothing, more: false };
      },
      commitPull: async (_c, token) => void calls.push(`commitPull ${token}`),
      push: async () => {
        calls.push("push");
        pushes += 1;
        return { accepted: 0, replaced: nothing, token: null, error: null, needsPull: pushes === 1 };
      },
      commitPush: async () => {},
    };
    await pushCollection(fake, collection(calls));
    expect(calls).toEqual(["read", "push", "read", "notice", "pull", "commitPull p1", "read", "push"]);
  });

  it("reports a refused entry even when nothing was replaced", async () => {
    const refusal = { code: "error.syncRecordTooLarge" };
    const { fake, calls } = backend([], { accepted: 0, replaced: nothing, token: null, error: refusal, needsPull: false });
    await expect(pushCollection(fake, collection(calls))).rejects.toEqual(refusal);
    expect(calls).toEqual(["read", "push"]);
  });
});

describe("the loop", () => {
  beforeEach(() => void vi.useFakeTimers());
  afterEach(() => void vi.useRealTimers());

  function harness(collections: SyncableCollection[]) {
    const { fake, calls } = backend([]);
    let focus = () => {};
    let request = () => {};
    const errors: unknown[] = [];
    const ends: RunResult[] = [];
    const kinds: string[] = [];
    const stop = startSyncLoop({
      backend: fake,
      collections: () => collections,
      onFocus: (listener) => {
        focus = listener;
        return () => {
          focus = () => {};
        };
      },
      onRequest: (listener) => {
        request = listener;
        return () => {
          request = () => {};
        };
      },
      onReplaced: () => {},
      onError: (_id, error) => void errors.push(error),
      onRunStart: (run) => void kinds.push(run),
      onRunEnd: (result) => void ends.push(result),
    });
    return {
      calls,
      errors,
      ends,
      kinds,
      starts: () => kinds.length,
      stop,
      focus: () => focus(),
      request: () => request(),
    };
  }

  it("says which kind of run is starting, so a push can pass unseen", async () => {
    const loop = harness([collection([])]);
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(FOCUS_COALESCE_MS);
    loop.focus();
    await vi.advanceTimersByTimeAsync(0);
    loop.stop();
    expect(loop.kinds).toEqual(["full", "push"]);
  });

  it("runs once for a burst of focus events, as one restore of the window fires", async () => {
    // WebView2 fires `focus` twice when a minimised window comes back; each asked for a push, the
    // second queued behind the first with nothing new to do.
    const loop = harness([collection([])]);
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(FOCUS_COALESCE_MS);
    loop.focus();
    loop.focus();
    await vi.advanceTimersByTimeAsync(0);
    loop.stop();
    expect(loop.kinds).toEqual(["full", "push"]);
  });

  it("runs again for a focus that comes after the burst", async () => {
    const loop = harness([collection([])]);
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(FOCUS_COALESCE_MS);
    loop.focus();
    await vi.advanceTimersByTimeAsync(FOCUS_COALESCE_MS);
    loop.focus();
    await vi.advanceTimersByTimeAsync(0);
    loop.stop();
    expect(loop.kinds).toEqual(["full", "push", "push"]);
  });

  it("pulls and pushes at launch, and on focus", async () => {
    const calls: string[] = [];
    const loop = harness([collection(calls)]);
    await vi.advanceTimersByTimeAsync(0);
    loop.focus();
    await vi.advanceTimersByTimeAsync(0);
    loop.stop();
    // Two runs: the full one reads twice, to notice and to push; the push inside a minute, once.
    expect(calls.filter((call) => call === "read")).toHaveLength(3);
  });

  it("between those, only pushes — and pulls again once a window has been quiet long enough", async () => {
    const shared = backend([]);
    const calls = shared.calls;
    const stop = startSyncLoop({
      backend: shared.fake,
      collections: () => [collection(calls)],
      onFocus: () => () => {},
      onRequest: () => () => {},
      onReplaced: () => {},
      onError: () => {},
    });
    await vi.advanceTimersByTimeAsync(0);
    calls.length = 0;
    await vi.advanceTimersByTimeAsync(LOCAL_CHECK_MS);
    expect(calls).toEqual(["read", "push"]);
    calls.length = 0;
    await vi.advanceTimersByTimeAsync(IDLE_PULL_MS);
    stop();
    expect(calls).toContain("pull");
  });

  it("keeps going past a collection that failed", async () => {
    const calls: string[] = [];
    const broken: SyncableCollection = {
      ...collection(calls),
      id: "broken",
      read: async () => {
        throw new Error("unreadable");
      },
    };
    const loop = harness([broken, collection(calls)]);
    await vi.advanceTimersByTimeAsync(0);
    loop.stop();
    expect(loop.errors).toHaveLength(1);
    expect(calls).toContain("read");
  });

  it("stays quiet while signed out", async () => {
    const calls: string[] = [];
    const signedOut = { code: "error.syncNotSignedIn" };
    const errors: unknown[] = [];
    const stop = startSyncLoop({
      backend: { ...backend([]).fake, pullPage: () => Promise.reject(signedOut) },
      collections: () => [collection(calls), collection(calls)],
      onFocus: () => () => {},
      onRequest: () => () => {},
      onReplaced: () => {},
      onError: (_id, error) => void errors.push(error),
    });
    await vi.advanceTimersByTimeAsync(0);
    stop();
    expect(errors).toEqual([]);
  });

  it("runs once more after a run that was asked for again, not once per ask", async () => {
    const calls: string[] = [];
    const loop = harness([collection(calls)]);
    loop.focus();
    loop.focus();
    loop.focus();
    await vi.advanceTimersByTimeAsync(0);
    loop.stop();
    // Two full runs — the asks came before the first had started, so no pull was recent — each
    // reading twice, to notice and to push.
    expect(calls.filter((call) => call === "read")).toHaveLength(4);
  });

  it("never lets two loops run at once, and a stopped one ends at the next collection", async () => {
    let active = 0;
    let most = 0;
    let reads = 0;
    const slow = (id: string): SyncableCollection => ({
      ...collection([]),
      id,
      read: async () => {
        reads += 1;
        active += 1;
        most = Math.max(most, active);
        await new Promise((resolve) => setTimeout(resolve, 100));
        active -= 1;
        return [];
      },
    });
    const both = [slow("a"), slow("b")];
    const start = () =>
      startSyncLoop({
        backend: backend([]).fake,
        collections: () => both,
        onFocus: () => () => {},
        onRequest: () => () => {},
        onReplaced: () => {},
        onError: () => {},
      });

    // What a remount does: the first loop is stopped while its run is still in `a`.
    const first = start();
    await vi.advanceTimersByTimeAsync(0);
    first();
    const second = start();
    await vi.advanceTimersByTimeAsync(1_000);
    second();

    expect(most).toBe(1);
    // Three collection runs, each full: read once to notice, once to push.
    expect(reads).toBe(6);
  });

  it("pulls on focus only once a minute has passed since the last pull", async () => {
    const shared = backend([]);
    let focus = () => {};
    const stop = startSyncLoop({
      backend: shared.fake,
      collections: () => [collection(shared.calls)],
      onFocus: (listener) => {
        focus = listener;
        return () => {};
      },
      onRequest: () => () => {},
      onReplaced: () => {},
      onError: () => {},
    });
    await vi.advanceTimersByTimeAsync(0);
    shared.calls.length = 0;

    focus();
    await vi.advanceTimersByTimeAsync(0);
    expect(shared.calls).toEqual(["read", "push"]);

    shared.calls.length = 0;
    await vi.advanceTimersByTimeAsync(FOCUS_PULL_MS);
    shared.calls.length = 0;
    focus();
    await vi.advanceTimersByTimeAsync(0);
    stop();
    expect(shared.calls).toContain("pull");
  });

  it("pulls whenever it is asked to, however recent the last pull", async () => {
    const shared = backend([]);
    let request = () => {};
    const stop = startSyncLoop({
      backend: shared.fake,
      collections: () => [collection(shared.calls)],
      onFocus: () => () => {},
      onRequest: (listener) => {
        request = listener;
        return () => {};
      },
      onReplaced: () => {},
      onError: () => {},
    });
    await vi.advanceTimersByTimeAsync(0);
    shared.calls.length = 0;
    request();
    await vi.advanceTimersByTimeAsync(0);
    stop();
    expect(shared.calls).toContain("pull");
  });

  it("reports each run's start and end, with its first failure", async () => {
    const calls: string[] = [];
    const broken: SyncableCollection = {
      ...collection(calls),
      id: "broken",
      read: async () => {
        throw new Error("unreadable");
      },
    };
    const loop = harness([broken, collection(calls)]);
    await vi.advanceTimersByTimeAsync(0);
    loop.stop();
    expect(loop.starts()).toBe(1);
    expect(loop.ends).toHaveLength(1);
    expect(loop.ends[0]).toMatchObject({ run: "full", finished: true });
    expect((loop.ends[0].error as Error).message).toBe("unreadable");
  });

  it("reports a clean run with no error", async () => {
    const calls: string[] = [];
    const loop = harness([collection(calls)]);
    await vi.advanceTimersByTimeAsync(0);
    loop.stop();
    expect(loop.ends).toEqual([{ run: "full", error: undefined, finished: true }]);
  });

  it("reports nothing at all when no row is on", async () => {
    const loop = harness([]);
    await vi.advanceTimersByTimeAsync(0);
    loop.focus();
    await vi.advanceTimersByTimeAsync(LOCAL_CHECK_MS);
    loop.stop();
    expect(loop.starts()).toBe(0);
    expect(loop.ends).toEqual([]);
  });

  it("asks once per full run, and pulls only what the answer names", async () => {
    const shared = backend([], undefined, async () => ["d"]);
    const calls = shared.calls;
    const stop = startSyncLoop({
      backend: shared.fake,
      collections: () => [collection(calls), collection(calls, undefined, "d")],
      onFocus: () => () => {},
      onRequest: () => () => {},
      onReplaced: () => {},
      onError: () => {},
    });
    await vi.advanceTimersByTimeAsync(0);
    stop();
    expect(calls.filter((call) => call === "heads")).toHaveLength(1);
    expect(shared.pulled).toEqual(["d"]);
    expect(calls.filter((call) => call === "push")).toHaveLength(2);
  });

  it("a failed heads fails the run and touches no collection", async () => {
    const shared = backend([], undefined, () => Promise.reject({ code: "error.syncServerRefused" }));
    const errors: unknown[] = [];
    const ends: RunResult[] = [];
    const stop = startSyncLoop({
      backend: shared.fake,
      collections: () => [collection(shared.calls)],
      onFocus: () => () => {},
      onRequest: () => () => {},
      onReplaced: () => {},
      onError: (_id, error) => void errors.push(error),
      onRunEnd: (result) => void ends.push(result),
    });
    await vi.advanceTimersByTimeAsync(0);
    stop();
    expect(shared.calls).toEqual(["heads"]);
    expect(errors).toHaveLength(1);
    expect(ends[0]?.error).toEqual({ code: "error.syncServerRefused" });
  });

  it("stays quiet when heads says signed out", async () => {
    const shared = backend([], undefined, () => Promise.reject({ code: "error.syncNotSignedIn" }));
    const errors: unknown[] = [];
    const stop = startSyncLoop({
      backend: shared.fake,
      collections: () => [collection(shared.calls)],
      onFocus: () => () => {},
      onRequest: () => () => {},
      onReplaced: () => {},
      onError: (_id, error) => void errors.push(error),
    });
    await vi.advanceTimersByTimeAsync(0);
    stop();
    expect(errors).toEqual([]);
    expect(shared.calls).toEqual(["heads"]);
  });
});

describe("the upload signal", () => {
  beforeEach(() => void vi.useFakeTimers());
  afterEach(() => void vi.useRealTimers());

  /** A loop whose backend hands each push's `onSending` to `sending` instead of calling it. */
  function uploadHarness(sending: (onSending: () => void) => void) {
    const { fake } = backend([]);
    const heard: string[] = [];
    const stop = startSyncLoop({
      backend: {
        ...fake,
        push: async (collection, items, onSending) => {
          if (onSending) sending(onSending);
          return fake.push(collection, items);
        },
      },
      collections: () => [collection([])],
      onFocus: () => () => {},
      onRequest: () => () => {},
      onReplaced: () => {},
      onError: () => {},
      onRunStart: (run) => void heard.push(`start ${run}`),
      onUploading: () => void heard.push("uploading"),
      onRunEnd: ({ run }) => void heard.push(`end ${run}`),
    });
    return { heard, stop };
  }

  it("passes a push's sending on, inside its run", async () => {
    const loop = uploadHarness((onSending) => onSending());
    await vi.advanceTimersByTimeAsync(0);
    loop.stop();
    expect(loop.heard).toEqual(["start full", "uploading", "end full"]);
  });

  it("drops a sending that arrives after its run ended", async () => {
    // A channel message is not ordered against its call's answer; one arriving late would put the
    // upload icon up with no run left to take it down.
    const late: (() => void)[] = [];
    const loop = uploadHarness((onSending) => void late.push(onSending));
    await vi.advanceTimersByTimeAsync(0);
    for (const onSending of late) onSending();
    loop.stop();
    expect(loop.heard).toEqual(["start full", "end full"]);
  });
});
