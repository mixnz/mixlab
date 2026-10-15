import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AppError } from "../../core/errors";
import type { SshConfig } from "../../core/ssh";
import type { ToolStatus, ToolSuite } from "./tools";
import type { ConnectionConfig } from "./types";

/**
 * The commands and events of this module that belong to no one engine: opening and closing a
 * connection, its tunnel, a dump or restore in progress, the tools those run, and a connection
 * another program handed over.
 *
 * What each engine asks once connected is its own `api.ts` — `mysql/`, `postgres/`, `mssql/`,
 * `clickhouse/`, `sqlite/`, `mongo/`, `redis/`. Between them, these are the only files of the
 * module that call `invoke`; `npm run lint` rejects it anywhere else. The credential store is
 * `core/secrets.ts`, shared with the other modules.
 */

// ---------------------------------------------------------------------------------------------
// A connection

/** Dials `config` and resolves with the id every later command names it by. */
export function connectDb(config: ConnectionConfig): Promise<string> {
  return invoke<string>("connect_db", { config });
}

/** Hangs up. The backend forgetting an id it never had is an error callers may ignore. */
export function disconnectDb(id: string): Promise<void> {
  return invoke<void>("disconnect_db", { id });
}

/** Opens the SSH session alone and closes it again — what the form's Test tunnel button asks. */
export function testSshTunnel(ssh: SshConfig): Promise<void> {
  return invoke<void>("test_ssh_tunnel", { ssh });
}

/** The password MixEngine keeps under `key` in its own keyring, or `null` when it no longer has
 *  one. See `withResolvedPassword` for when this may be asked. */
export function resolveMixEngineSecret(key: string): Promise<string | null> {
  return invoke<string | null>("secrets_resolve_mixengine", { key });
}

// ---------------------------------------------------------------------------------------------
// A connection handed over

/**
 * A connection another program handed to MixLab — MixEngine's `mix database open` — as it is
 * taken by the tab opened for it.
 *
 * The backend read it off a `mixlab://connect?…` URL and the password off one environment
 * variable, and keeps it until this call; see
 * `docs/specs/2026-09-03-mixengine-connection-handoff-design.md`. From here it is a
 * config in the form like any typed by hand: `connect_db`, the workspace, Save if the user wants.
 */
export interface Handoff {
  config: ConnectionConfig;
  /** The tab's name and the name pre-filled for saving — the launcher's own id for the server. */
  label: string;
  /** The address to save instead of a copy of the password — see `SavedConnection.keyringRef`.
   *  `null`/absent for a `mixlab://` link that arrived without proof MixEngine started this
   *  process, even when the URL itself named one. */
  keyring_ref?: string | null;
}

/** Takes the handoff under `id`. Rejects with `error.handoffExpired` when it was already taken,
 *  or when the id is from a session an earlier run wrote — in which case the tab is simply empty. */
export function takeHandoff(id: string): Promise<Handoff> {
  return invoke("handoff_take", { id });
}

// ---------------------------------------------------------------------------------------------
// The SSH tunnel

/**
 * What is happening to a connection's SSH tunnel.
 *
 * Only connections going through a tunnel have this event — a direct connection emits nothing, so
 * no separate flag is needed to know whether to draw the banner.
 */
export interface TunnelState {
  /** Which tab this connection belongs to: two tabs may drop at the same moment and each one only
   *  listens to its own. */
  id: string;
  state: "reconnecting" | "reconnected" | "failed";
  /** Only present with `failed`: why it could not be reopened — a wrong key, an unreachable
   *  host. */
  error?: AppError;
}

/** Listens to all tunnel news for `id` until the returned function is called. */
export function onTunnelState(
  id: string,
  onState: (state: TunnelState) => void
): Promise<UnlistenFn> {
  return listen<TunnelState>("tunnel://state", ({ payload }) => {
    if (payload.id === id) onState(payload);
  });
}

/** Reopens the SSH session right away, instead of waiting out the backoff of the Rust-side
 *  watcher. */
export function tunnelReconnect(id: string): Promise<void> {
  return invoke("tunnel_reconnect", { id });
}

// ---------------------------------------------------------------------------------------------
// A dump or restore

/**
 * How far a running dump or restore has got, as the Rust side works it out.
 *
 * The two are not measured the same way and the numbers should not be read as if they were. A
 * restore's is a count: the file is of a known size and every byte of it is fed to the client by
 * MixLab. A dump's is an estimate, built from the tables mysqldump says it has reached and the size
 * of the file it is writing — the tool keeps no count of its own to ask for.
 */
export interface TransferProgress {
  /** The connection this is about: two tabs can be transferring at once, and each overlay wants
   *  its own. */
  id: string;
  /** 0 to 99, or null when there is no honest percentage to show. Never 100: what says a transfer
   *  is finished is the call that started it returning. */
  percent: number | null;
  /** What is being written out at this moment — a table of a MySQL database, a collection of a
   *  MongoDB one. A restore has none: what it is replaying is a stream, not a list of anything. */
  part: string | null;
  /** Which of them that is, counted from one: the one in hand, not the ones behind it. Zero when
   *  there is none to be on, which is a restore and the moment before a dump reaches its first. */
  atPart: number;
  parts: number;
}

/** Reports every reading of `id`'s transfer until the returned function is called. */
export function onTransferProgress(
  id: string,
  onProgress: (progress: TransferProgress) => void
): Promise<UnlistenFn> {
  return listen<TransferProgress>("transfer://progress", ({ payload }) => {
    if (payload.id === id) onProgress(payload);
  });
}

/**
 * Stops the tool a connection is running, if it is running one.
 *
 * The tool is killed rather than asked to stop: `mysqldump` and the rest have no protocol for
 * finishing early. So a cancelled dump leaves a partial file behind — the command reports being
 * stopped rather than reporting success, which is what says the file is not a dump.
 *
 * Never rejects for a connection that is transferring nothing, which is what lets a tab closing
 * call it without first working out whether it needs to.
 */
export function cancelTransfer(id: string): Promise<void> {
  return invoke("cancel_db_transfer", { id });
}

// ---------------------------------------------------------------------------------------------
// The dump tools. `tools.ts` is what the screens call: it keeps an install's progress across them.

export function dumpToolsStatus(): Promise<ToolStatus[]> {
  return invoke<ToolStatus[]>("dumptools_status");
}

export function dumpToolsReady(suite: ToolSuite): Promise<boolean> {
  return invoke<boolean>("dumptools_ready", { suite });
}

export function dumpToolsDownloadable(suite: ToolSuite): Promise<boolean> {
  return invoke<boolean>("dumptools_downloadable", { suite });
}

/** Reports its progress as `tools://progress` events, which `tools.ts` listens to. */
export function dumpToolsInstall(suite: ToolSuite): Promise<void> {
  return invoke<void>("dumptools_install", { suite });
}

export function dumpToolsUninstall(suite: ToolSuite): Promise<void> {
  return invoke<void>("dumptools_uninstall", { suite });
}

export function dumpToolsSetPath(tool: string, path: string | null): Promise<void> {
  return invoke<void>("dumptools_set_path", { tool, path });
}
