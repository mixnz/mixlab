import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createActivity, SHOW_AFTER_MS, SHOW_AT_LEAST_MS, syncedAgo } from "./activity";

describe("the direction icon", () => {
  beforeEach(() => void vi.useFakeTimers());
  afterEach(() => void vi.useRealTimers());

  const ended = (run: "full" | "push") => ({ run, error: undefined, finished: true });

  it("never shows for a download shorter than the delay", () => {
    const store = createActivity();
    store.downloading();
    vi.advanceTimersByTime(SHOW_AFTER_MS - 1);
    store.runEnded(ended("full"));
    vi.advanceTimersByTime(SHOW_AT_LEAST_MS);
    expect(store.get().direction).toBeNull();
  });

  it("shows down for a long download, and stays at least the minimum once shown", () => {
    const store = createActivity();
    store.downloading();
    vi.advanceTimersByTime(SHOW_AFTER_MS);
    expect(store.get().direction).toBe("down");
    store.runEnded(ended("full"));
    vi.advanceTimersByTime(SHOW_AT_LEAST_MS - 1);
    expect(store.get().direction).toBe("down");
    vi.advanceTimersByTime(1);
    expect(store.get().direction).toBeNull();
  });

  it("keeps its icon through back-to-back runs", () => {
    const store = createActivity();
    store.downloading();
    vi.advanceTimersByTime(SHOW_AFTER_MS);
    store.runEnded(ended("full"));
    store.downloading();
    vi.advanceTimersByTime(SHOW_AT_LEAST_MS * 2);
    expect(store.get().direction).toBe("down");
  });

  it("never shows for a full run that only asks, however long it takes", () => {
    // T189: a full run first asks the server what changed. Asking is not downloading, and a run
    // told that nothing changed only pushes.
    const store = createActivity();
    vi.advanceTimersByTime(SHOW_AFTER_MS * 10);
    store.runEnded(ended("full"));
    vi.advanceTimersByTime(SHOW_AT_LEAST_MS);
    expect(store.get().direction).toBeNull();
    expect(store.get().lastSyncedAt).not.toBeNull();
  });

  it("never shows for a push that sends nothing, however long it takes", () => {
    // A push is the local check alt-tabbing runs: reading every collection, sending nothing when
    // nothing changed. Measured at 240–465ms with eleven rows on, so a delay alone does not hide it.
    const store = createActivity();
    vi.advanceTimersByTime(SHOW_AFTER_MS * 10);
    expect(store.get().direction).toBeNull();
    store.runEnded(ended("push"));
    expect(store.get().direction).toBeNull();
  });

  it("shows up at once when a push sends, and holds it the minimum", () => {
    const store = createActivity();
    store.uploading();
    expect(store.get().direction).toBe("up");
    store.runEnded(ended("push"));
    vi.advanceTimersByTime(SHOW_AT_LEAST_MS - 1);
    expect(store.get().direction).toBe("up");
    vi.advanceTimersByTime(1);
    expect(store.get().direction).toBeNull();
  });

  it("turns a full run's down into up once it sends, after down has had its minimum", () => {
    const store = createActivity();
    store.downloading();
    vi.advanceTimersByTime(SHOW_AFTER_MS);
    store.uploading();
    expect(store.get().direction).toBe("down");
    vi.advanceTimersByTime(SHOW_AT_LEAST_MS);
    expect(store.get().direction).toBe("up");
  });

  it("goes straight to up when a full run sends before down was due", () => {
    const store = createActivity();
    store.downloading();
    store.uploading();
    expect(store.get().direction).toBe("up");
    vi.advanceTimersByTime(SHOW_AFTER_MS);
    expect(store.get().direction).toBe("up");
  });

  it("lets a full run's icon stop on time when a push follows it", () => {
    const store = createActivity();
    store.downloading();
    vi.advanceTimersByTime(SHOW_AFTER_MS);
    store.runEnded(ended("full"));
    store.runEnded(ended("push"));
    vi.advanceTimersByTime(SHOW_AT_LEAST_MS);
    expect(store.get().direction).toBeNull();
  });

  it("tells its subscribers, and hands out the same value until something changes", () => {
    const store = createActivity();
    const heard = vi.fn();
    store.subscribe(heard);
    const before = store.get();
    expect(store.get()).toBe(before);
    store.downloading();
    vi.advanceTimersByTime(SHOW_AFTER_MS);
    expect(heard).toHaveBeenCalled();
    expect(store.get()).not.toBe(before);
  });
});

describe("what the last run leaves behind", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000_000);
  });
  afterEach(() => void vi.useRealTimers());

  it("records when a full run finished cleanly", () => {
    const store = createActivity();
    store.runEnded({ run: "full", error: undefined, finished: true });
    expect(store.get()).toMatchObject({ lastSyncedAt: 1_000_000, lastError: undefined });
  });

  it("does not count a push, or a run that stopped early, as a sync", () => {
    const store = createActivity();
    store.runEnded({ run: "push", error: undefined, finished: true });
    store.runEnded({ run: "full", error: undefined, finished: false });
    expect(store.get().lastSyncedAt).toBeNull();
  });

  it("keeps a failure until a full run succeeds", () => {
    const store = createActivity();
    const failure = new Error("offline");
    store.runEnded({ run: "full", error: failure, finished: true });
    store.runEnded({ run: "push", error: undefined, finished: true });
    expect(store.get().lastError).toBe(failure);
    store.runEnded({ run: "full", error: undefined, finished: true });
    expect(store.get().lastError).toBeUndefined();
  });
});

describe("syncedAgo", () => {
  const minute = 60_000;

  it("says nothing under a minute, so the caller can say just now", () => {
    expect(syncedAgo(0, 59_000, "en")).toBeNull();
    expect(syncedAgo(10_000, 0, "en")).toBeNull();
  });

  it("counts minutes, then hours, then days, in the app's language", () => {
    expect(syncedAgo(0, 2 * minute, "en")).toBe("2 minutes ago");
    expect(syncedAgo(0, 3 * 60 * minute, "en")).toBe("3 hours ago");
    expect(syncedAgo(0, 2 * 24 * 60 * minute, "en")).toBe("2 days ago");
    expect(syncedAgo(0, 2 * minute, "vi")).toBe("2 phút trước");
  });
});
