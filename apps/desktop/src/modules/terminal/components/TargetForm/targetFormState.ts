import { DEFAULT_SSH_PORT } from "../../../../core/ssh";
import { stableStringify } from "../../../../core/stableStringify";
import { openingFor } from "../../session";
import { formatEnvLines, parseEnvLines, parsePathLines } from "../../targetEnv";
import type { LocalShell, OnRestore, SavedTarget, SshAuth, SshConfig, TerminalChoice } from "../../types";

/** What the form says about a shell on this machine. */
export interface LocalDraft {
  /* The name rather than the path, both in state and as the `Select`'s value: the name is what goes
     to disk, so a saved row can be loaded into the form even before the machine's shell list has
     finished reading. */
  shellName: string;
  cwd: string;
  /* What the shell is given beside its directory, as the lines the person types. */
  envText: string;
  pathText: string;
  /* The two fields above sit under *Advanced*, closed: most targets set neither, and a form that
     grows by two boxes for everyone is a form whose buttons fall off the pane. Open whenever there
     is something in them, so nothing a target sets is hidden from the person editing it. */
  advanced: boolean;
}

/** What the form says about a server. */
export interface SshDraft {
  host: string;
  port: number;
  username: string;
  authType: "password" | "privatekey";
  password: string;
  keyPath: string;
  passphrase: string;
}

/** Every field of the form. Both halves are kept while the other kind is on screen. */
export interface Draft {
  kind: "local" | "ssh";
  name: string;
  runOnConnect: string;
  /** What a tab restored on the next launch does with *Run on connect*. Not part of a
   *  session, so it is read off the saved entry once that is found. */
  onRestore: OnRestore;
  local: LocalDraft;
  ssh: SshDraft;
}

/**
 * The saved entry the form holds, if any.
 *
 * `snapshot` is the entry as it was loaded, so the Update button knows whether anything has
 * changed; `null` while the entry a tab came back with has not been looked up yet.
 * `changedElsewhere` is that entry changing somewhere else while the form held edits. `removed` is
 * the entry deleted somewhere else: the form holds nothing, and says so until it is saved or the
 * notice is dismissed.
 */
export type Held =
  | { kind: "none" }
  | { kind: "removed" }
  | { kind: "held"; id: string; snapshot: string | null; changedElsewhere: boolean };

export interface FormState {
  draft: Draft;
  held: Held;
}

export type FormAction =
  | { type: "edit"; patch: Partial<Pick<Draft, "kind" | "name" | "runOnConnect" | "onRestore">> }
  | { type: "editLocal"; patch: Partial<LocalDraft> }
  | { type: "editSsh"; patch: Partial<SshDraft> }
  /** The shell and directory the settings name, into fields still empty. */
  | { type: "defaults"; shellName: string; cwd: string | undefined }
  /** A saved row loaded into the form. */
  | { type: "apply"; entry: SavedTarget }
  /** The entry the tab came back with, looked up once the list is read; `undefined` when gone. */
  | { type: "named"; entry: SavedTarget | undefined }
  /** "+": a new one of the kind on screen. */
  | { type: "clear" }
  /** The form was saved as `entry`. */
  | { type: "saved"; entry: SavedTarget }
  | { type: "changedElsewhere" }
  | { type: "removedElsewhere" }
  | { type: "dismissRemoved" };

const EMPTY_SSH: SshDraft = {
  host: "",
  port: DEFAULT_SSH_PORT,
  username: "",
  authType: "password",
  password: "",
  keyPath: "",
  passphrase: "",
};

/**
 * A saved target reduced to exactly "what it is" — without `id`.
 *
 * Leaving out `id` is what makes *Save as new* comparable: the new entry has a different id but
 * identical content, and the Update button has to read "nothing changed" right after. It goes
 * through `stableStringify` because the key order of a freshly built form object and of the same
 * object after a round trip through JSON are not the same — see `core/stableStringify.ts`.
 */
export function snapshotOf(target: SavedTarget): string {
  const { id: _id, ...body } = target;
  return stableStringify(body);
}

/** The form a tab starts with. `initial` is the tab that was just tried and failed: the form
 *  rebuilds exactly what the user typed. */
export function initialForm(initial: TerminalChoice | null): FormState {
  const local = initial?.kind === "local" ? initial : null;
  const ssh = initial?.kind === "ssh" ? initial.config : null;
  const envText = local ? formatEnvLines(local.env ?? undefined) : "";
  const pathText = local ? (local.pathPrepend ?? []).join("\n") : "";
  return {
    draft: {
      kind: initial?.kind ?? "local",
      name: "",
      runOnConnect: initial?.runOnConnect ?? "",
      onRestore: "run",
      local: { shellName: local?.shell.name ?? "", cwd: local?.cwd ?? "", envText, pathText, advanced: envText !== "" || pathText !== "" },
      ssh: ssh
        ? {
            host: ssh.host,
            port: ssh.port,
            username: ssh.username,
            authType: ssh.auth.type,
            password: ssh.auth.type === "password" ? ssh.auth.password : "",
            keyPath: ssh.auth.type === "privatekey" ? ssh.auth.key_path : "",
            passphrase: ssh.auth.type === "privatekey" ? (ssh.auth.passphrase ?? "") : "",
          }
        : EMPTY_SSH,
    },
    held: initial?.targetId ? { kind: "held", id: initial.targetId, snapshot: null, changedElsewhere: false } : { kind: "none" },
  };
}

function holding(entry: SavedTarget): Held {
  return { kind: "held", id: entry.id, snapshot: snapshotOf(entry), changedElsewhere: false };
}

export function targetForm(state: FormState, action: FormAction): FormState {
  const { draft } = state;
  switch (action.type) {
    case "edit":
      return { ...state, draft: { ...draft, ...action.patch } };
    case "editLocal":
      return { ...state, draft: { ...draft, local: { ...draft.local, ...action.patch } } };
    case "editSsh":
      return { ...state, draft: { ...draft, ssh: { ...draft.ssh, ...action.patch } } };
    case "defaults":
      return {
        ...state,
        draft: {
          ...draft,
          local: {
            ...draft.local,
            shellName: draft.local.shellName || action.shellName,
            cwd: draft.local.cwd || (action.cwd ?? ""),
          },
        },
      };
    case "apply": {
      const { entry } = action;
      // The targets column is always there, even while the form is on the other kind — clicking a
      // row without the form switching kind would look like the click did nothing.
      const common = { kind: entry.kind, name: entry.name, runOnConnect: entry.runOnConnect ?? "", onRestore: entry.onRestore ?? "run" };
      if (entry.kind === "local") {
        return {
          held: holding(entry),
          /* And the other branch cleared. Left as it was, switching to the SSH tab afterwards would
             show the address and password of another server — the one loaded before — while the
             left column highlights a local row, and the Update button stands ready to turn that row
             into a server. */
          draft: {
            ...common,
            local: {
              shellName: entry.shellName,
              cwd: entry.cwd ?? "",
              envText: formatEnvLines(entry.env),
              pathText: (entry.pathPrepend ?? []).join("\n"),
              advanced: entry.env !== undefined || entry.pathPrepend !== undefined,
            },
            ssh: EMPTY_SSH,
          },
        };
      }
      const { config } = entry;
      return {
        held: holding(entry),
        draft: {
          ...common,
          // Symmetrically: another local row's starting directory has no business here any more.
          // The shell name stays — it belongs to no row; it is what the "This machine" tab opens by
          // default.
          local: { ...draft.local, cwd: "", envText: "", pathText: "" },
          ssh: {
            host: config.host,
            port: config.port,
            username: config.username,
            authType: config.auth.type,
            password: config.auth.type === "password" ? config.auth.password : "",
            keyPath: config.auth.type === "privatekey" ? config.auth.key_path : "",
            passphrase: config.auth.type === "privatekey" ? (config.auth.passphrase ?? "") : "",
          },
        },
      };
    }
    case "named":
      /* Only the name, the restore choice and the snapshot, deliberately not the whole entry: what
         the user just typed has to stay as it is. A form that, after an opening failed on a wrong
         password, puts the old password back by itself is a form arguing with the user. And because
         the snapshot is taken from the entry rather than from the form, a just-edited password
         brings the Update button back to life. An entry deleted while the session ran leaves the
         form as it is, pointing at nothing. */
      if (action.entry === undefined) return { ...state, held: { kind: "none" } };
      return {
        draft: { ...draft, name: action.entry.name, onRestore: action.entry.onRestore ?? "run" },
        held: holding(action.entry),
      };
    case "clear":
      // The selected kind stays: "+" means "a new one of the kind I am looking at". So does the
      // shell, which belongs to no row.
      return {
        held: { kind: "none" },
        draft: {
          ...draft,
          name: "",
          runOnConnect: "",
          onRestore: "run",
          local: { ...draft.local, cwd: "", envText: "", pathText: "" },
          ssh: EMPTY_SSH,
        },
      };
    case "saved":
      return { ...state, held: holding(action.entry) };
    case "changedElsewhere":
      return state.held.kind === "held" ? { ...state, held: { ...state.held, changedElsewhere: true } } : state;
    case "removedElsewhere":
      return { ...state, held: { kind: "removed" } };
    case "dismissRemoved":
      return state.held.kind === "removed" ? { ...state, held: { kind: "none" } } : state;
  }
}

/** The id of the saved entry the form holds, or `null` when it was typed by hand. */
export function heldId(held: Held): string | null {
  return held.kind === "held" ? held.id : null;
}

/** The variables as typed, or the first line that is not `KEY=value`: such a form neither saves
 *  nor opens, and says which line. */
export function draftEnv(draft: Draft): {
  envError: number | null;
  env: Record<string, string> | undefined;
  pathPrepend: string[];
} {
  const parsed = parseEnvLines(draft.local.envText);
  return {
    envError: "error" in parsed ? parsed.error : null,
    env: "env" in parsed && Object.keys(parsed.env).length > 0 ? parsed.env : undefined,
    pathPrepend: parsePathLines(draft.local.pathText),
  };
}

function buildConfig({ ssh }: Draft): SshConfig {
  const auth: SshAuth =
    ssh.authType === "password"
      ? { type: "password", password: ssh.password }
      : { type: "privatekey", key_path: ssh.keyPath, passphrase: ssh.passphrase || undefined };
  return { host: ssh.host.trim(), port: ssh.port, username: ssh.username.trim(), auth };
}

/** Which target the form describes, under the id passed in. An empty field is written as
 *  `undefined` rather than `""`: absence is the default, so an entry not using that field does
 *  not carry a dead line around. `onRestore` likewise: `run` is what an absent one means. */
export function buildTarget(draft: Draft, id: string): SavedTarget {
  const name = draft.name.trim();
  const opening = draft.runOnConnect.trim() || undefined;
  const restore = draft.onRestore === "run" ? {} : { onRestore: draft.onRestore };
  if (draft.kind === "local") {
    const { env, pathPrepend } = draftEnv(draft);
    return {
      id,
      name,
      kind: "local",
      shellName: draft.local.shellName,
      cwd: draft.local.cwd.trim() || null,
      runOnConnect: opening,
      ...restore,
      ...(env === undefined ? {} : { env }),
      ...(pathPrepend.length === 0 ? {} : { pathPrepend }),
    };
  }
  return { id, name, kind: "ssh", config: buildConfig(draft), runOnConnect: opening, ...restore };
}

/** Enough for an attempt to mean something: an address, a user, and whatever the chosen
 *  authentication method needs. */
function sshReady({ ssh }: Draft): boolean {
  return (
    ssh.host.trim() !== "" &&
    ssh.username.trim() !== "" &&
    (ssh.authType === "password" ? ssh.password !== "" : ssh.keyPath.trim() !== "")
  );
}

/** What is being opened, built from the form. `null` when the form is not yet enough for an
 *  attempt to mean something. */
export function buildChoice(draft: Draft, shells: LocalShell[], targetId: string | null): TerminalChoice | null {
  const opening = draft.runOnConnect.trim() || null;
  if (draft.kind === "local") {
    const shell = shells.find((s) => s.name === draft.local.shellName);
    const { envError, env, pathPrepend } = draftEnv(draft);
    return shell && envError === null
      ? {
          kind: "local",
          shell,
          cwd: draft.local.cwd.trim() || null,
          targetId,
          ...openingFor(opening, draft.onRestore),
          env: env ?? null,
          pathPrepend: pathPrepend.length === 0 ? null : pathPrepend,
        }
      : null;
  }
  return sshReady(draft)
    ? { kind: "ssh", config: buildConfig(draft), targetId, ...openingFor(opening, draft.onRestore) }
    : null;
}

/** Enough to save: a name, for this machine a real shell whose name can be saved, and variables
 *  that read. */
export function isSavable(draft: Draft, shells: LocalShell[]): boolean {
  return (
    draft.name.trim() !== "" &&
    (draft.kind === "ssh" ||
      (shells.some((s) => s.name === draft.local.shellName) && draftEnv(draft).envError === null))
  );
}
