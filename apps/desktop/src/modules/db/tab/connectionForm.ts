import { DEFAULT_PORTS, type ConnectionConfig, type DbKind, type SshConfig } from "../types";
import type { TranslationKey } from "../../../i18n";

/**
 * Everything the connection form is holding, as one value.
 *
 * Seventeen `useState` calls before this, which is why it is worth a file: loading a saved
 * connection and starting a new one each called seventeen setters in a fixed order, and the two
 * lists had to be kept in step by hand. A field added to one and forgotten in the other is not a
 * type error — it is a form that keeps the last connection's SSH passphrase after the user asks
 * for a new connection.
 *
 * The two directions are here rather than in the component because they are the part that can be
 * wrong, and the part worth testing: what a saved config reads as, and what the form says back.
 */
export interface ConnectionForm {
  kind: DbKind;
  host: string;
  port: number;
  username: string;
  password: string;
  database: string;
  /** Mongo's whole connection string. Empty for every other kind. */
  uri: string;
  /** SQLite's database file. Empty for every other kind. */
  path: string;
  /** Whether the connection string is shown. A connection string is only editable once shown, and
   *  showing it puts a password on screen — so an empty one starts open (there is nothing to
   *  protect yet) and a saved one starts hidden. */
  uriRevealed: boolean;
  confirmingReveal: boolean;
  useSsl: boolean;
  tunnelType: "direct" | "ssh";
  sshHost: string;
  sshPort: number;
  sshUser: string;
  sshAuthType: "password" | "privatekey";
  sshPassword: string;
  sshKeyPath: string;
  sshPassphrase: string;
  /** The reference `password` was resolved from, when it was — see `SavedConnection.keyringRef`.
   *  Carried on the form rather than in `ConnectionConfig` because it is a fact about *saving*,
   *  not about the connection itself. Cleared the moment `password` is edited by hand: at that
   *  point what is in the box is no longer MixEngine's, and Save must not go on writing the old
   *  address over it. */
  keyringRef: string | null;
}

/** The SSH half at rest. Written once and spread into both directions below, so that a field added
 *  here reaches the new-connection form and the loading of a saved direct connection together. */
const NO_TUNNEL = {
  tunnelType: "direct",
  sshHost: "",
  sshPort: 22,
  sshUser: "",
  sshAuthType: "password",
  sshPassword: "",
  sshKeyPath: "",
  sshPassphrase: "",
} as const satisfies Partial<ConnectionForm>;

/**
 * The form a saved connection reads as, or — given `null` — the form a new connection starts on.
 *
 * One function for both because they are the same question: everything not carried by the config
 * has to come back to its default, and a `null` config carries nothing.
 *
 * `keyringRef` is not part of `config` — it comes from wherever `config` itself came from (a
 * handoff, a saved connection) and is passed in separately.
 */
export function formFrom(config: ConnectionConfig | null, keyringRef: string | null = null): ConnectionForm {
  if (config === null) {
    return {
      kind: "mysql",
      host: "127.0.0.1",
      port: DEFAULT_PORTS.mysql,
      username: "",
      password: "",
      database: "",
      uri: "",
      path: "",
      uriRevealed: true,
      confirmingReveal: false,
      // Off to start with: a new connection is most often to a local or tunnelled server, where
      // SSL buys nothing and an old server's TLS config is one more thing to fail on.
      useSsl: false,
      ...NO_TUNNEL,
      keyringRef: null,
    };
  }

  const ssh = config.ssh;
  return {
    kind: config.kind,
    host: config.host,
    port: config.port,
    username: config.username ?? "",
    password: config.password ?? "",
    database: config.database ?? "",
    uri: config.uri ?? "",
    path: config.path ?? "",
    uriRevealed: !config.uri,
    confirmingReveal: false,
    keyringRef,
    /* Absent means "prefer TLS" to the backend (`use_ssl == Some(false)` is the only thing that
       turns it off), which is why an entry from before this box existed reads as on. But only for
       a kind that has the box at all: `configFrom` writes `undefined` for Mongo and Redis, so
       loading one of those and then switching the form to MySQL used to arrive with TLS silently
       ticked — a form the user never set that way. */
    useSsl: hasTls(config.kind) ? config.use_ssl ?? true : false,
    ...NO_TUNNEL,
    ...(ssh
      ? {
          tunnelType: "ssh" as const,
          sshHost: ssh.host,
          sshPort: ssh.port,
          sshUser: ssh.username,
          sshAuthType: ssh.auth.type,
          ...(ssh.auth.type === "password"
            ? { sshPassword: ssh.auth.password }
            : { sshKeyPath: ssh.auth.key_path, sshPassphrase: ssh.auth.passphrase ?? "" }),
        }
      : {}),
  };
}

/** The tunnel the form describes, or nothing when it describes a direct connection. */
function sshFrom(form: ConnectionForm): SshConfig | undefined {
  if (form.tunnelType !== "ssh") return undefined;
  return {
    host: form.sshHost,
    port: form.sshPort,
    username: form.sshUser,
    auth:
      form.sshAuthType === "password"
        ? { type: "password", password: form.sshPassword }
        : {
            type: "privatekey",
            key_path: form.sshKeyPath,
            passphrase: form.sshPassphrase || undefined,
          },
  };
}

/** What the form says, as the config to connect or save with. */
export function configFrom(form: ConnectionForm): ConnectionConfig {
  const isMongo = form.kind === "mongo";
  const isSqlite = form.kind === "sqlite";
  return {
    kind: form.kind,
    host: form.host,
    port: form.port,
    // Mongo takes its endpoint, credentials and default database from the connection string, so
    // the per-field values are left out entirely rather than saved as dead weight.
    /* SQLite takes the file and nothing else — there is no server to have an account on, and no
       address to tunnel to — so its fields are left out rather than saved as dead weight, the way
       Mongo's per-field values are. */
    username: isMongo || isSqlite ? undefined : form.username || undefined,
    password: isMongo || isSqlite ? undefined : form.password || undefined,
    database: isMongo || isSqlite ? undefined : form.database || undefined,
    uri: isMongo ? form.uri.trim() || undefined : undefined,
    path: isSqlite ? form.path.trim() || undefined : undefined,
    ssh: isSqlite ? undefined : sshFrom(form),
    use_ssl: hasTls(form.kind) ? form.useSsl : undefined,
  };
}

/** The form with a different kind picked, and the port that kind is normally on. Changing the kind
 *  is the one field that moves another: a port left at MySQL's 3306 while the tab says Postgres is
 *  a connection that fails for a reason the form is showing but nobody reads. */
export function withKind(form: ConnectionForm, kind: DbKind): ConnectionForm {
  return { ...form, kind, port: DEFAULT_PORTS[kind] };
}

/** What each kind is called on the tab that picks it, and — read aloud — beside its logo in the
 *  saved list, where the logo itself says nothing to a screen reader. */
export const KIND_LABEL: Record<DbKind, TranslationKey> = {
  mysql: "connection.kindMysql",
  postgres: "connection.kindPostgres",
  mongo: "connection.kindMongo",
  redis: "connection.kindRedis",
  sqlite: "connection.kindSqlite",
  clickhouse: "connection.kindClickhouse",
  mssql: "connection.kindMssql",
};

/**
 * `KIND_LABEL[kind]`, but doesn't crash when `kind` is a value *this build's* `DbKind` doesn't
 * actually list. `SavedConnection.config.kind` is only *typed* as `DbKind` when read from
 * `connections.json` — nothing verifies it at runtime — so a connection saved by a newer version,
 * read back by this one, can carry a kind that isn't in the table.
 *
 * See docs/specs/2026-09-05-unknown-db-kind-crash-and-error-logging-design.md.
 */
export function kindLabel(kind: DbKind): TranslationKey {
  return (KIND_LABEL as Partial<Record<string, TranslationKey>>)[kind] ?? "connection.kindUnknown";
}

/**
 * Whether this kind has a TLS box on the form at all.
 *
 * Not `isSqlKind`, which this used to borrow. The two agreed while every SQL engine was a server,
 * and stopped agreeing the moment SQLite became one: a file has no transport to secure, so the box
 * would be a control that changes nothing. ClickHouse is back to being a server — `use_ssl` there
 * picks `https://` over `http://` outright rather than negotiating TLS inside one connection the
 * way MySQL and PostgreSQL do, but the box asks the same question a user has an answer to either
 * way: does reaching this server need encryption. SQL Server is a fourth spelling of the same
 * question — it encrypts the login packet whatever this says, so the box decides whether the rest
 * of the session is encrypted too.
 */
function hasTls(kind: DbKind): boolean {
  return (
    kind === "mysql" || kind === "postgres" || kind === "clickhouse" || kind === "mssql"
  );
}
