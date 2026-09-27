import type { SyncChanges, SyncableCollection } from "../../core/syncCollection";
import type { SyncBackend } from "./api";

/**
 * How often this machine looks, locally, for something it has not lent yet. A look that finds
 * nothing costs no request — Rust compares hashes before it opens a socket — so this is how *"on a
 * local change, after a debounce"* (D8, rule 1) is met without every module announcing its writes.
 */
export const LOCAL_CHECK_MS = 30_000;

/** How long a window goes without news before it asks for some anyway (D8, rule 1). */
export const IDLE_PULL_MS = 15 * 60_000;

/**
 * How long after a full run a window regaining focus pulls again. A full run is one request per
 * row that is on, and a person alt-tabbing is not news: inside this, focus only pushes, which asks
 * nothing of the server when nothing changed.
 */
export const FOCUS_PULL_MS = 60_000;

/**
 * Focus events closer together than this are one. Restoring a minimised window fires `focus` twice
 * in WebView2, and each asked for its own run.
 */
export const FOCUS_COALESCE_MS = 1_000;

function isEmpty(changes: SyncChanges): boolean {
  return changes.upserts.length === 0 && changes.removed.length === 0;
}

/**
 * One collection, both ways: this machine's changes noticed, every page pulled and written, then
 * those changes pushed.
 * Resolves to how many of this machine's edits newer ones replaced (D4). `onSending` is the push's:
 * see {@link pushCollection}.
 *
 * **Nothing is committed before the module has written it.** A write that throws leaves its page,
 * or a lost conflict's winner, uncommitted — so the next run meets it again, rather than recording
 * an agreement the disk does not hold.
 */
export async function syncCollection(
  backend: SyncBackend,
  collection: SyncableCollection,
  onSending?: () => void,
): Promise<number> {
  // Stamped before the first page, so a pull weighs this machine's changes rather than writing
  // over them (D4) — its own earlier push, coming back, included.
  await backend.notice(collection.id, await collection.read());
  for (;;) {
    const page = await backend.pullPage(collection.id);
    const skipped = isEmpty(page.changes) ? [] : await collection.write(page.changes);
    await backend.commitPull(collection.id, page.token, skipped);
    if (!page.more) break;
  }
  return pushCollection(backend, collection, true, onSending);
}

/**
 * This machine's changes alone — what the local check runs, without asking the server for news.
 * A collection this account never pulled is synced in full instead: a push trusts that what this
 * machine never saw the server does not have, which holds only after a pull.
 * `onSending` is called when something is about to leave for the server, which most pushes never do.
 */
export async function pushCollection(
  backend: SyncBackend,
  collection: SyncableCollection,
  pulled = false,
  onSending?: () => void,
): Promise<number> {
  const pushed = await backend.push(collection.id, await collection.read(), onSending);
  if (pushed.needsPull) {
    // Just pulled and still never pulled: the cursor was not recorded, and going round again would
    // not record it either.
    if (pulled) throw new Error(`sync: ${collection.id} was pulled but has no cursor`);
    return syncCollection(backend, collection, onSending);
  }
  if (pushed.token !== null) {
    const skipped = isEmpty(pushed.replaced) ? [] : await collection.write(pushed.replaced);
    await backend.commitPush(collection.id, pushed.token, skipped);
  }
  // After the commit: what landed and what was replaced are recorded, and only then is the refusal
  // reported, the way a failed push always was (T178c, C2).
  if (pushed.error !== null) throw pushed.error;
  return pushed.replaced.upserts.length + pushed.replaced.removed.length;
}

export interface LoopOptions {
  backend: SyncBackend;
  /** The collections that are on, asked at every run so turning one on takes effect at the next. */
  collections: () => SyncableCollection[];
  /** Subscribes to the window gaining focus; returns the unsubscribe. */
  onFocus: (listener: () => void) => () => void;
  /** Subscribes to a run being asked for (`requestSync`), which always pulls; returns the unsubscribe. */
  onRequest: (listener: () => void) => () => void;
  /** A run began: at least one row is on. */
  onRunStart?: (run: Run) => void;
  /**
   * That full run is about to pull: the server named something that changed elsewhere (T189).
   * Never for a run that only asked and was told nothing changed.
   */
  onDownloading?: () => void;
  /** That run is about to send this machine's changes — only ever between its start and its end. */
  onUploading?: () => void;
  /** That run ended, however it ended. */
  onRunEnd?: (result: RunResult) => void;
  /** Edits made here that newer ones replaced — for a notice, never a question (D4). */
  onReplaced: (collectionId: string, count: number) => void;
  onError: (collectionId: string, error: unknown) => void;
}

export type Run = "full" | "push";

/** How one run went, for whoever draws it (`activity.ts`). */
export interface RunResult {
  run: Run;
  /** The first collection's failure, or `undefined` when none failed. */
  error: unknown;
  /** False when it stopped early: signed out, or its loop was stopped. */
  finished: boolean;
}

/**
 * The one lane every loop's runs take, whichever loop they belong to. A window that remounts its
 * workspace (StrictMode, a hot reload) stops one loop and starts another while the first is still
 * writing; two runs applying the same page each see an id missing and each add it.
 */
let lane: Promise<void> = Promise.resolve();

function inLane(run: () => Promise<void>): Promise<void> {
  const next = lane.then(run, run);
  lane = next.catch(() => {});
  return next;
}

/** Signed out: every collection would say the same, and none of it is news. */
function isSignedOut(error: unknown): boolean {
  return (
    typeof error === "object" && error !== null && (error as { code?: unknown }).code === "error.syncNotSignedIn"
  );
}

/**
 * Sync at D8's moments: at launch, on focus once {@link FOCUS_PULL_MS} has passed, whenever
 * asked, a local check every {@link LOCAL_CHECK_MS}, and a pull when nothing has been heard for
 * {@link IDLE_PULL_MS}. A full run asks the server once which collections changed elsewhere
 * (T189) and pulls only those. **One run at a time**, across every
 * loop there is: a moment that arrives during a run asks for one more, however many arrive, and a
 * loop started while a stopped one is still writing waits for it. Returns the stop.
 */
export function startSyncLoop(options: LoopOptions): () => void {
  let running = false;
  let queued: Run | null = null;
  let stopped = false;
  let lastFull = 0;
  let lastFocus = -Infinity;
  let runs = 0;
  let current: number | null = null;

  async function runOnce(run: Run): Promise<void> {
    const collections = options.collections();
    // With every row off there is nothing to do and nothing to show.
    if (collections.length === 0) return;
    if (run === "full") lastFull = Date.now();
    const id = ++runs;
    current = id;
    // A channel's message is not ordered against the call it came with: one landing after its run
    // ended would show an upload nothing is doing, and nothing would ever take it down.
    const onSending = () => {
      if (current === id) options.onUploading?.();
    };
    options.onRunStart?.(run);
    let error: unknown = undefined;
    let finished = false;
    try {
      // One question for the whole run (T189): a collection no other machine touched is only
      // pushed. A push run never asks, as it never did.
      let stale: Set<string> | null = null;
      if (run === "full") {
        try {
          stale = new Set(await options.backend.heads(collections.map((collection) => collection.id)));
        } catch (failure) {
          if (isSignedOut(failure)) return;
          error = failure;
          options.onError("heads", failure);
          return;
        }
        if (stale.size > 0) options.onDownloading?.();
      }
      for (const collection of collections) {
        // A stopped loop finishes the collection it is in, and starts no other.
        if (stopped) return;
        try {
          const replaced = stale?.has(collection.id)
            ? await syncCollection(options.backend, collection, onSending)
            : await pushCollection(options.backend, collection, false, onSending);
          if (replaced > 0) options.onReplaced(collection.id, replaced);
        } catch (failure) {
          if (isSignedOut(failure)) return;
          if (error === undefined) error = failure;
          options.onError(collection.id, failure);
        }
      }
      finished = true;
    } finally {
      current = null;
      options.onRunEnd?.({ run, error, finished });
    }
  }

  function ask(run: Run): void {
    if (stopped) return;
    if (running) {
      queued = queued === "full" || run === "full" ? "full" : "push";
      return;
    }
    running = true;
    void (async () => {
      let next: Run | null = run;
      while (next !== null && !stopped) {
        const run = next;
        await inLane(() => runOnce(run));
        next = queued;
        queued = null;
      }
      running = false;
    })();
  }

  ask("full");
  const unfocus = options.onFocus(() => {
    // One restore of a minimised window fires `focus` twice; the second has nothing new to ask.
    if (Date.now() - lastFocus < FOCUS_COALESCE_MS) return;
    lastFocus = Date.now();
    ask(Date.now() - lastFull >= FOCUS_PULL_MS ? "full" : "push");
  });
  const unrequest = options.onRequest(() => ask("full"));
  const timer = setInterval(() => ask(Date.now() - lastFull >= IDLE_PULL_MS ? "full" : "push"), LOCAL_CHECK_MS);
  return () => {
    stopped = true;
    unfocus();
    unrequest();
    clearInterval(timer);
  };
}
