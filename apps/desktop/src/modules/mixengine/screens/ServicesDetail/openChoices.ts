import type { DatabaseClientReport, DesktopClient } from "@mixengine/api";

/**
 * The module a database service opens into.
 *
 * The one id this module names, and the frontend half of `open_in_mixdb.rs`'s `module_id: "db"` —
 * a bridge between two modules is by definition one naming the other.
 */
export const DATABASE_MODULE_ID = "db";

/** One *open* control on the Services screen. */
export type OpenChoice =
  /** Open a tab of the built-in client, which this window is already drawing. */
  | "builtIn"
  /** The same, and say first that it turns the client on. */
  | "builtInAfterEnabling";

/**
 * What the *open* affordance is, for one service — T110's D4.
 *
 * Drawn from `database.client`: `no_client` is a state and not an error, which the panel renders as
 * the sentence it always did rather than as a failure of something the person did.
 *
 * **Never another application** (T165): the only client the daemon names is this install's own
 * window, so every choice opens in-process. Handing the service back through `database.open` would
 * start a second MixLab to forward a URL to this one.
 */
export function openChoices(client: DesktopClient, builtInVisible: boolean): OpenChoice[] {
  if (client.state !== "installed") return [];
  return [builtInVisible ? "builtIn" : "builtInAfterEnabling"];
}

/**
 * Whether it is a service a database client can open.
 *
 * **`protocol` is the answer, and it is a state, not an error** — `database.client` returns `null`
 * for nginx, caddy and every php-fpm pool, just as it returns a protocol for postgres. This turns
 * that state into "draw nothing at all": the panel on the Services screen and the three-dot menu
 * on the Dashboard ask the same question, so that question is defined only once.
 *
 * Absent also means no, under [ADR 0019]: `protocol` is an optional member, and absence means this
 * daemon is older than that member — not "could not be determined".
 *
 * [ADR 0019]: https://github.com/mixnz/mixlab/blob/master/docs/decisions/0019-an-added-response-member-is-optional.md
 */
export function opensADatabase(report: Partial<Pick<DatabaseClientReport, "protocol">>): boolean {
  return report.protocol !== null && report.protocol !== undefined;
}

/**
 * Whether to draw the Create database form for this service — roadmap task T155.
 *
 * **The answer is the daemon's**: `creates_databases: false` for Redis and MongoDB, servers that do
 * not create databases this way, so the form there could only be refused. Absence means a daemon
 * older than that member ([ADR 0019]) — keep the form as before.
 *
 * [ADR 0019]: https://github.com/mixnz/mixlab/blob/master/docs/decisions/0019-an-added-response-member-is-optional.md
 */
export function createsDatabases(
  report: Partial<Pick<DatabaseClientReport, "creates_databases">>,
): boolean {
  return report.creates_databases !== false;
}
