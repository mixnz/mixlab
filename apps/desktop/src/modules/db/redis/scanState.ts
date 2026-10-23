import { REDIS_FIRST_CURSOR, type RedisKeyInfo } from "./api";

/**
 * The sidebar's walk of the keyspace: what it has read, where `SCAN` resumes, whether the cursor has
 * come full circle, and whether a sweep or one more page is out.
 *
 * Which sweep an answer belongs to is not in here: `RedisWorkspace`'s `scanRun` decides that before
 * anything is dispatched, so a page of an abandoned sweep never reaches this.
 */
export interface Scan {
  keys: RedisKeyInfo[];
  cursor: string;
  done: boolean;
  loading: boolean;
  loadingMore: boolean;
}

export const SCAN_IDLE: Scan = { keys: [], cursor: REDIS_FIRST_CURSOR, done: false, loading: false, loadingMore: false };

export type ScanAction =
  /** A sweep from the top. A page still out belongs to the sweep just abandoned, and nothing will
   *  clear its `loadingMore` for it. */
  | { type: "started" }
  /** What the sweep has so far, every key it read. */
  | { type: "published"; keys: RedisKeyInfo[]; cursor: string; done: boolean }
  | { type: "finished" }
  | { type: "moreAsked" }
  /** One more page past the ceiling, merged by name: `SCAN` can hand the same key back twice. */
  | { type: "moreLanded"; keys: RedisKeyInfo[]; cursor: string; done: boolean }
  | { type: "moreEnded" }
  | { type: "keysDeleted"; names: string[] };

export function scanReducer(scan: Scan, action: ScanAction): Scan {
  switch (action.type) {
    case "started":
      return { ...SCAN_IDLE, loading: true };
    case "published":
      return { ...scan, keys: action.keys, cursor: action.cursor, done: action.done };
    case "finished":
      return { ...scan, loading: false };
    case "moreAsked":
      return { ...scan, loadingMore: true };
    case "moreLanded": {
      const seen = new Set(scan.keys.map((k) => k.name));
      return {
        ...scan,
        keys: [...scan.keys, ...action.keys.filter((k) => !seen.has(k.name))],
        cursor: action.cursor,
        done: action.done,
      };
    }
    case "moreEnded":
      return { ...scan, loadingMore: false };
    case "keysDeleted": {
      const gone = new Set(action.names);
      return { ...scan, keys: scan.keys.filter((k) => !gone.has(k.name)) };
    }
  }
}
