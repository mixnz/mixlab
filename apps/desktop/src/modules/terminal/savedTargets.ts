import { Store } from "@tauri-apps/plugin-store";
import { invoke } from "@tauri-apps/api/core";
import type { SavedTarget, SshConfig } from "./types";
import { dedupeById, upsertById } from "../../core/byId";
import { mergeSshSecrets, splitSshSecrets, type SshSecrets } from "../../core/ssh";

/**
 * The list of saved targets, split across two places.
 *
 * `terminal-hosts.json` holds what a target *is* — the name, and depending on the kind: the shell
 * and its starting directory, or the address, port, user and key path — and it is plain text on
 * purpose: it is the list of places you open often, and being able to read and copy it is handy.
 * What opens the door goes into the operating system's credential store, through the three
 * `secrets_*` commands the db module also uses.
 *
 * `runOnConnect` stays in the file along with the name: it is a few startup command lines, not a
 * secret — and the caption under that field in the form says so plainly, because someone who
 * thinks it is hidden will put `export TOKEN=…` in it.
 *
 * The `local` branch has nothing to hide, so it does not touch the credential store on any of the
 * three paths: save, read and delete.
 *
 * The file name is kept from when the list only held servers. Renaming the file would leave behind
 * the list of everyone using the old version, and nobody ever sees that name.
 *
 * The id is a uuid this module generates, so it never collides with a database connection's id —
 * the two share one store but share no key.
 *
 * Where the split happens is this file's own business: what goes in and comes out is a complete
 * `SavedTarget`.
 */

let storePromise: Promise<Store> | null = null;

function getStore(): Promise<Store> {
  if (!storePromise) {
    storePromise = Store.load("terminal-hosts.json");
  }
  return storePromise;
}

/* What must not sit in `terminal-hosts.json`, and how to split it out and merge it back:
   `core/ssh.ts`, because the db module splits exactly the same thing for its tunnel. The old names
   are kept for the places that already import them. */
export type HostSecrets = SshSecrets;

/**
 * An entry on disk, read defensively, or `null` when it is not an entry at all.
 *
 * Everything arriving here is JSON some version of the app wrote — including a version that did
 * not know what `kind` is. An entry missing `kind` reads as `ssh`: the list only held servers back
 * then, so that is not a guess.
 *
 * An absent `cwd` and a `null` `cwd` are the same thing, just as in `tabState.ts`: open the shell
 * in its default directory.
 */
export function parseSavedTarget(value: unknown): SavedTarget | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const entry = value as Record<string, unknown>;
  if (typeof entry.id !== "string" || entry.id === "") return null;
  if (typeof entry.name !== "string") return null;
  const runOnConnect = typeof entry.runOnConnect === "string" ? entry.runOnConnect : undefined;

  if (entry.kind === "local") {
    if (typeof entry.shellName !== "string" || entry.shellName === "") return null;
    const cwd = typeof entry.cwd === "string" ? entry.cwd : null;
    return { id: entry.id, name: entry.name, kind: "local", shellName: entry.shellName, cwd, runOnConnect };
  }

  // `undefined` also lands here: see the comment on the function.
  if (entry.kind !== undefined && entry.kind !== "ssh") return null;
  const config = parseSshConfig(entry.config);
  if (config === null) return null;
  return { id: entry.id, name: entry.name, kind: "ssh", config, runOnConnect };
}

/**
 * The `config` part of an ssh entry, read field by field, or `null` when nothing is left to draw.
 *
 * This used to be an `as SshConfig` — a promise to the compiler, not a check. A hand-edited entry
 * missing `auth` slipped through, then `mergeSecrets` read `config.auth.type` and threw in the
 * middle of `loadSavedTargets`' `Promise.all`: **the whole list** vanished for the entire session,
 * exactly the opposite of what `loadStored` promises just above it.
 *
 * Only `host` cannot be guessed — without it there is no row to draw. Everything else has a sound
 * default, by the same reasoning this file chose elsewhere: a server for which the secret store
 * has nothing left still shows up with an empty password field. A missing or unknown `auth` reads
 * as an empty password for the same reason — that row can be fixed in the form, while throwing it
 * away loses the name and address too.
 */
function parseSshConfig(value: unknown): SshConfig | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const config = value as Record<string, unknown>;
  if (typeof config.host !== "string" || config.host === "") return null;

  const port = config.port;
  const auth = typeof config.auth === "object" && config.auth !== null ? (config.auth as Record<string, unknown>) : {};

  return {
    host: config.host,
    // The default ssh port. An entry that does not state a port can still open; one stating a
    // wrong one cannot.
    port: typeof port === "number" && Number.isInteger(port) && port > 0 && port <= 65535 ? port : 22,
    username: typeof config.username === "string" ? config.username : "",
    auth:
      auth.type === "privatekey"
        ? {
            type: "privatekey",
            key_path: typeof auth.key_path === "string" ? auth.key_path : "",
            passphrase: typeof auth.passphrase === "string" ? auth.passphrase : undefined,
          }
        : {
            type: "password",
            // Empty is normal, not broken: `splitSecrets` writes exactly that, and the real
            // password lives in the operating system's store.
            password: typeof auth.password === "string" ? auth.password : "",
          },
  };
}

/** What gets written to the file: a target with its secret part taken out. The `local` branch
 *  passes through intact. */
export function withoutSecrets(target: SavedTarget): SavedTarget {
  return target.kind === "ssh" ? { ...target, config: splitSshSecrets(target.config).config } : target;
}

export function saveSecrets(id: string, secrets: HostSecrets): Promise<void> {
  return invoke<void>("secrets_save", { id, secrets });
}

export function loadSecrets(id: string): Promise<HostSecrets> {
  return invoke<HostSecrets>("secrets_load", { id });
}

/** Forgets a host's credentials: when it goes, and when sync says another machine cleared them. */
export function deleteSecrets(id: string): Promise<void> {
  return invoke<void>("secrets_delete", { id });
}

/**
 * What is really on disk, with the secret part taken out beforehand.
 *
 * An entry that cannot be read drops out of the list rather than breaking the whole read — and
 * since every write passes through here first, it also disappears from the file on the next save.
 * That is right: a row that cannot be drawn cannot be opened either, and keeping it is just keeping
 * something broken. What the old version wrote can be read — a missing `kind` is `ssh`; see
 * `parseSavedTarget`.
 */
async function loadStored(): Promise<SavedTarget[]> {
  const store = await getStore();
  const raw = (await store.get<unknown[]>("hosts")) ?? [];
  const parsed = raw.map(parseSavedTarget).filter((entry): entry is SavedTarget => entry !== null);
  // Two sync runs at once could each add the same host (fixed in the loop); a file written then
  // still holds both, and this is where it heals.
  const once = dedupeById(parsed, (entry) => entry.id);
  if (once !== parsed) {
    await store.set("hosts", once.map(withoutSecrets));
    await store.save();
  }
  return once;
}

async function persist(list: SavedTarget[]): Promise<void> {
  const store = await getStore();
  await store.set("hosts", list.map(withoutSecrets));
  await store.save();
}

/** Every saved target, with the secret part merged back in. A server for which the store has
 *  nothing left still comes back here — just with an empty password field, and that is the right
 *  thing to show. */
export async function loadSavedTargets(): Promise<SavedTarget[]> {
  const stored = await loadStored();
  return Promise.all(
    stored.map(async (target) =>
      target.kind === "ssh"
        ? { ...target, config: mergeSshSecrets(target.config, await loadSecrets(target.id)) }
        : target,
    ),
  );
}

/**
 * Writes `target`'s secrets into the operating system's store, then the whole list — with no
 * secrets — to the file. The other targets are stripped too: they were just handed out with their
 * secret parts merged in.
 *
 * The `local` branch writes an empty set rather than skipping entirely, and that is the easy place
 * to slip: a row that used to be a server, edited into a shell on this machine, without passing
 * through here would leave its old password in the operating system's store forever —
 * `removeSavedTarget` would later look at a `local` entry and have no reason to delete anything.
 * An empty set is a delete command; see `secrets.rs`.
 */
async function persistTarget(list: SavedTarget[], target: SavedTarget): Promise<void> {
  await saveSecrets(target.id, target.kind === "ssh" ? splitSshSecrets(target.config).secrets : {});
  await persist(list);
}

export async function addSavedTarget(target: SavedTarget): Promise<SavedTarget[]> {
  const list = await loadSavedTargets();
  // An id already here is replaced rather than repeated: sync adds by id, and may be late to learn
  // that it already did.
  const next = upsertById(list, target, (entry) => entry.id);
  await persistTarget(next, target);
  return next;
}

export async function updateSavedTarget(target: SavedTarget): Promise<SavedTarget[]> {
  const list = await loadSavedTargets();
  const next = list.map((entry) => (entry.id === target.id ? target : entry));
  await persistTarget(next, target);
  return next;
}

export async function removeSavedTarget(id: string): Promise<SavedTarget[]> {
  const list = await loadSavedTargets();
  const next = list.filter((entry) => entry.id !== id);
  /* Secrets go with the target they belong to; leaving them behind leaves an entry in the operating
     system's store that nothing names any more. Asked unconditionally, even for a shell on this
     machine: deleting what is not there is not an error (`secrets.rs`), while guessing it never had
     anything is wrong in exactly one case — a row that used to be a server. */
  await deleteSecrets(id);
  await persist(next);
  return next;
}
