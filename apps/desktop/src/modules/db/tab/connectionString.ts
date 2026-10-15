import type { ConnectionForm } from "./connectionForm";
import type { ConnectionConfig, DbKind, SavedConnection } from "../types";

/** The scheme each server engine's URLs are written with. SQLite has a path and Mongo a URI of its
 *  own, so neither is here. */
const SCHEME: Record<Exclude<DbKind, "sqlite" | "mongo">, string> = {
  mysql: "mysql",
  postgres: "postgresql",
  redis: "redis",
  clickhouse: "clickhouse",
  mssql: "sqlserver",
};

/** The `user:password@` part of a Mongo URI, split so the password alone can be covered. */
const MONGO_USERINFO_RE = /^(mongodb(?:\+srv)?:\/\/)([^@/?]+)@/i;

/**
 * The address the form describes, as one line somebody can read and copy — and **never with a
 * password in it**. A line on screen with a copy button beside it is exactly the thing that ends up
 * pasted into a chat, so the one secret the form holds stays out of it.
 *
 * - A server engine: `scheme://user@host:port/database`, each part left out when the form has none.
 * - SQLite: the file's path.
 * - MongoDB: the URI as entered, its password replaced by `***`.
 *
 * The host and port are what is dialled, which through a tunnel is the server as the SSH host sees
 * it — the same values the form sends, so the line agrees with what Connect will do.
 */
export function connectionString(form: ConnectionForm): string {
  if (form.kind === "sqlite") return form.path.trim();
  if (form.kind === "mongo") return maskMongoPassword(form.uri.trim());

  const scheme = (SCHEME as Partial<Record<string, string>>)[form.kind] ?? form.kind;
  const user = form.username.trim();
  const host = form.host.trim();
  const database = form.database.trim();
  const authority = `${user === "" ? "" : `${user}@`}${host}${form.port > 0 ? `:${form.port}` : ""}`;
  return `${scheme}://${authority}${database === "" ? "" : `/${database}`}`;
}

/** `mongodb://user:secret@host` → `mongodb://user:***@host`. A URI with no password is returned as
 *  it is: there is nothing in it to cover. */
export function maskMongoPassword(uri: string): string {
  const match = MONGO_USERINFO_RE.exec(uri);
  if (match === null) return uri;
  const [full, scheme, userinfo] = match;
  const colon = userinfo.indexOf(":");
  if (colon < 0) return uri;
  return `${scheme}${userinfo.slice(0, colon)}:***@${uri.slice(full.length)}`;
}

/**
 * Where a connection points, in the few characters a list row or a route diagram has room for:
 * `host:port` for a server, the file's path for SQLite, and for MongoDB the hosts out of its URI —
 * never the credentials in front of them.
 */
export function connectionPlace(config: Pick<ConnectionConfig, "kind" | "host" | "port" | "path" | "uri">): string {
  if (config.kind === "sqlite") return config.path?.trim() ?? "";
  if (config.kind === "mongo") {
    const uri = config.uri?.trim() ?? "";
    const rest = uri.replace(/^mongodb(?:\+srv)?:\/\//i, "");
    const hosts = rest.slice(rest.indexOf("@") + 1);
    return hosts.split(/[/?]/, 1)[0] ?? "";
  }
  const host = config.host.trim();
  return config.port > 0 ? `${host}:${config.port}` : host;
}

/** What the saved-connection list is narrowed to: a name search, and the engines picked. An empty
 *  set of engines means every engine. */
export interface ConnectionFilter {
  query: string;
  kinds: ReadonlySet<DbKind>;
}

/** The saved connections the filter leaves, in the order they were given. */
export function filterConnections(
  list: readonly SavedConnection[],
  filter: ConnectionFilter,
): SavedConnection[] {
  const needle = filter.query.trim().toLocaleLowerCase();
  return list.filter(
    (entry) =>
      (filter.kinds.size === 0 || filter.kinds.has(entry.config.kind)) &&
      (needle === "" || entry.name.toLocaleLowerCase().includes(needle)),
  );
}

/** How many saved connections there are of each engine, for the chips — only engines that have one
 *  appear, in the order they are first met. */
export function engineCounts(list: readonly SavedConnection[]): [DbKind, number][] {
  const counts = new Map<DbKind, number>();
  for (const entry of list) counts.set(entry.config.kind, (counts.get(entry.config.kind) ?? 0) + 1);
  return [...counts.entries()];
}
