import type { EnvPair } from "../env/env";

/**
 * Splits and builds connection strings for the four kinds of DB MixLab supports.
 *
 * The tool opens no connection to check whether the string is right — it only reads and writes.
 */

export type DbKind = "mysql" | "postgres" | "mongodb" | "redis";

export interface ConnectionParam {
  key: string;
  value: string;
}

export interface ConnectionFields {
  kind: DbKind;
  /** Only meaningful for MongoDB: `mongodb+srv://` takes the host and port from the DNS SRV
   *  record. */
  srv: boolean;
  host: string;
  /** A string rather than a number: an empty field is "use the default", and `0` is not the way to
   *  say that. */
  port: string;
  user: string;
  password: string;
  database: string;
  params: ConnectionParam[];
}

export const DEFAULT_PORT: Record<DbKind, string> = {
  mysql: "3306",
  postgres: "5432",
  mongodb: "27017",
  redis: "6379",
};

const SCHEMES: Record<string, { kind: DbKind; srv: boolean }> = {
  "mysql:": { kind: "mysql", srv: false },
  "postgresql:": { kind: "postgres", srv: false },
  "postgres:": { kind: "postgres", srv: false },
  "mongodb:": { kind: "mongodb", srv: false },
  "mongodb+srv:": { kind: "mongodb", srv: true },
  "redis:": { kind: "redis", srv: false },
  "rediss:": { kind: "redis", srv: false },
};

const SCHEME_OF: Record<DbKind, string> = {
  mysql: "mysql",
  postgres: "postgresql",
  mongodb: "mongodb",
  redis: "redis",
};

/** `decodeURIComponent` throws on a broken percent sequence such as `%zz`. A hand-typed connection
 *  string may contain exactly that, and returning it verbatim is more useful than returning
 *  nothing. */
function safeDecode(text: string): string {
  try {
    return decodeURIComponent(text);
  } catch {
    return text;
  }
}

export function parseConnectionString(text: string): ConnectionFields | null {
  let url: URL;
  try {
    url = new URL(text.trim());
  } catch {
    return null;
  }
  const scheme = SCHEMES[url.protocol];
  if (!scheme) return null;

  return {
    kind: scheme.kind,
    srv: scheme.srv,
    host: url.hostname,
    port: scheme.srv ? "" : url.port,
    /* This is the silent failure: `url.username` and `url.password` return strings that are
       **already** percent-encoded, unlike `pathname` and `searchParams`. Forget to decode and the
       password field shows `p%40ss`, the user copies it into a config file, and the symptom at the
       other end is "wrong credentials". */
    user: safeDecode(url.username),
    password: safeDecode(url.password),
    database: url.pathname.replace(/^\//, ""),
    params: [...url.searchParams].map(([key, value]) => ({ key, value })),
  };
}

function query(params: ConnectionParam[]): string {
  const parts = params
    .filter((param) => param.key !== "")
    .map((param) => `${encodeURIComponent(param.key)}=${encodeURIComponent(param.value)}`);
  return parts.length === 0 ? "" : `?${parts.join("&")}`;
}

export function toUri(fields: ConnectionFields): string {
  const scheme = fields.kind === "mongodb" && fields.srv ? "mongodb+srv" : SCHEME_OF[fields.kind];
  /* Encode both the user and the password. `/`, `?` and `#` are required — without them the string
     cannot be parsed anywhere. `@` and `:` are still read correctly by `URL` thanks to its "split
     at the last `@`" rule, but this string also gets pasted into drivers, config files and human
     eyes, and not every one of them follows that rule. */
  const auth =
    fields.user === "" && fields.password === ""
      ? ""
      : `${encodeURIComponent(fields.user)}${
          fields.password === "" ? "" : `:${encodeURIComponent(fields.password)}`
        }@`;
  const port = fields.srv || fields.port === "" ? "" : `:${fields.port}`;
  const database = fields.database === "" ? "" : `/${fields.database}`;
  return `${scheme}://${auth}${fields.host}${port}${database}${query(fields.params)}`;
}

/** `null` for MongoDB and Redis: those two kinds **have no JDBC standard**, and printing a
 *  valid-looking `jdbc:mongodb://…` string hands the user something that will break somewhere
 *  else. */
export function toJdbc(fields: ConnectionFields): string | null {
  if (fields.kind !== "mysql" && fields.kind !== "postgres") return null;
  const driver = fields.kind === "mysql" ? "mysql" : "postgresql";
  const port = fields.port === "" ? DEFAULT_PORT[fields.kind] : fields.port;
  const auth: ConnectionParam[] = [];
  if (fields.user !== "") auth.push({ key: "user", value: fields.user });
  if (fields.password !== "") auth.push({ key: "password", value: fields.password });
  const all = [...auth, ...fields.params];
  return `jdbc:${driver}://${fields.host}:${port}/${fields.database}${query(all)}`;
}

/** Fixed `DB_*` names rather than the official docker images' variable names
 *  (`MYSQL_ROOT_PASSWORD`, `POSTGRES_USER`…): those names differ from image to image and version to
 *  version, while `DB_*` is predictable and fixing one line is all it takes. */
export function toEnvPairs(fields: ConnectionFields): EnvPair[] {
  return [
    { key: "DB_HOST", value: fields.host },
    { key: "DB_PORT", value: fields.port === "" ? DEFAULT_PORT[fields.kind] : fields.port },
    { key: "DB_USER", value: fields.user },
    { key: "DB_PASSWORD", value: fields.password },
    { key: "DB_NAME", value: fields.database },
  ];
}
