import { Store } from "@tauri-apps/plugin-store";
import { invoke } from "@tauri-apps/api/core";
import { dedupeById, upsertById } from "../../core/byId";
import { mergeSshSecrets, splitSshSecrets, type SshSecrets } from "../../core/ssh";
import type { ConnectionConfig, SavedConnection } from "./types";

/**
 * The saved connection list, split across two places.
 *
 * `connections.json` holds what a connection *is* — host, port, user, which database — and is
 * plain text on purpose: it is useful to be able to read, copy and edit that list. Everything that
 * would let someone connect goes to the OS credential store instead (Windows Credential Manager,
 * the macOS Keychain, the Secret Service on Linux), reached through the `secrets_*` commands.
 *
 * The split is invisible to callers: what goes in and comes out of this module is a whole
 * `SavedConnection`.
 */

let storePromise: Promise<Store> | null = null;

function getStore(): Promise<Store> {
  if (!storePromise) {
    storePromise = Store.load("connections.json");
  }
  return storePromise;
}

/** The fields that must never sit in `connections.json`: this module's two, and the SSH pair that
 *  every module tunnelling through a server has. */
interface Secrets extends SshSecrets {
  password?: string;
  uri?: string;
}

/**
 * Everything about `config` that is a credential.
 *
 * The whole Mongo connection string counts as one: it carries the password inside it, and taking
 * that out would mean rewriting a URI whose shape (a replica-set seed list, `+srv`, options) this
 * app deliberately doesn't parse for anything but cosmetics.
 *
 * `keyringRef` set means `config.password` is only ever a copy resolved from MixEngine's own
 * keyring entry to connect — it must not be written here too, or every Save would leave behind
 * exactly the duplicate the reference exists to avoid.
 */
export function readSecrets(config: ConnectionConfig, keyringRef?: string): Secrets {
  const secrets: Secrets = { ...(config.ssh ? splitSshSecrets(config.ssh).secrets : {}) };
  if (!keyringRef && config.password) secrets.password = config.password;
  if (config.uri) secrets.uri = config.uri;
  return secrets;
}

/** The same config with every credential taken out — what is written to disk. */
export function withoutSecrets(config: ConnectionConfig): ConnectionConfig {
  const stripped: ConnectionConfig = { ...config, password: undefined, uri: undefined };
  if (config.ssh) stripped.ssh = splitSshSecrets(config.ssh).config;
  return stripped;
}

/** The config as the form needs it: what was on disk, with the credentials put back. */
export function withSecrets(config: ConnectionConfig, secrets: Secrets): ConnectionConfig {
  const filled: ConnectionConfig = {
    ...config,
    password: secrets.password ?? config.password,
    uri: secrets.uri ?? config.uri,
  };
  if (config.ssh) filled.ssh = mergeSshSecrets(config.ssh, secrets);
  return filled;
}

export function saveSecrets(id: string, secrets: Secrets): Promise<void> {
  return invoke<void>("secrets_save", { id, secrets });
}

export function loadSecrets(id: string): Promise<Secrets> {
  return invoke<Secrets>("secrets_load", { id });
}

/** Forgets a connection's credentials: when it goes, and when sync says another machine cleared them. */
export function deleteSecrets(id: string): Promise<void> {
  return invoke<void>("secrets_delete", { id });
}

/**
 * `config` with the password `keyringRef` names filled in — what connecting needs, and nothing
 * else does. Asked only when the box is empty: a password typed over the reference, or one a
 * handoff brought along, is used as it is.
 *
 * This is the one place a saved connection reaches MixEngine's keyring. Listing, syncing and
 * saving never do, so a launch asks the Keychain about MixLab's own vault and nothing else
 * (ADR 0056). `undefined` when MixEngine no longer has the entry — a normal end, not an error:
 * the connection dials with no password, as one that never had one saved would.
 */
export async function withResolvedPassword(
  config: ConnectionConfig,
  keyringRef: string | null | undefined,
): Promise<ConnectionConfig> {
  if (!keyringRef || config.password) return config;
  const password = await invoke<string | null>("secrets_resolve_mixengine", { key: keyringRef });
  return { ...config, password: password ?? undefined };
}

/** Whether a tab restored as connected dials again at launch. Not one whose password is
 *  MixEngine's: that would be a second Keychain question before anybody asked to connect. It comes
 *  back as its form, and Connect resolves it. */
export function reconnectsOnRestore(entry: SavedConnection): boolean {
  return !entry.keyringRef;
}

/** What is actually on disk, credentials already removed. */
async function loadStored(): Promise<SavedConnection[]> {
  const store = await getStore();
  return (await store.get<SavedConnection[]>("saved")) ?? [];
}

async function persist(list: SavedConnection[]): Promise<void> {
  const store = await getStore();
  await store.set("saved", list);
  await store.save();
}

/**
 * Every saved connection, credentials included.
 *
 * A connection saved by a version of the standalone client that kept passwords in the file is moved across on the
 * way through: its credentials go to the credential store and the file is rewritten without them.
 * That happens once, silently — the user has nothing to decide here.
 */
export async function loadSavedConnections(): Promise<SavedConnection[]> {
  const read = await loadStored();
  // Two sync runs at once could each add the same connection (fixed in the loop); a file written
  // then still holds both, and this is where it heals.
  const stored = dedupeById(read, (entry) => entry.id);
  let needsRewrite = stored !== read;

  const list = await Promise.all(
    stored.map(async (entry) => {
      const kept = await loadSecrets(entry.id);
      const inFile = readSecrets(entry.config);
      // Anything still in the file was written before the credential store was used. It wins over
      // what is stored, being the newer of the two, and the file is rewritten without it below.
      if (Object.keys(inFile).length > 0) {
        needsRewrite = true;
        await saveSecrets(entry.id, { ...kept, ...inFile });
      }
      const config = withSecrets(entry.config, { ...kept, ...inFile });
      return { ...entry, config };
    }),
  );

  if (needsRewrite) await persist(list.map(stripEntry));
  return list;
}

/** One entry as it goes to disk. */
function stripEntry(entry: SavedConnection): SavedConnection {
  return { ...entry, config: withoutSecrets(entry.config) };
}

/** Writes `entry`'s credentials to the OS store and the whole list — credentials removed — to
 *  disk. The other entries are stripped too: they were handed out with theirs filled in. */
async function persistEntry(list: SavedConnection[], entry: SavedConnection): Promise<void> {
  await saveSecrets(entry.id, readSecrets(entry.config, entry.keyringRef));
  await persist(list.map(stripEntry));
}

export async function addSavedConnection(entry: SavedConnection): Promise<SavedConnection[]> {
  const list = await loadSavedConnections();
  // An id already here is replaced rather than repeated: sync adds by id, and may be late to learn
  // that it already did.
  const next = upsertById(list, entry, (c) => c.id);
  await persistEntry(next, entry);
  return next;
}

export async function updateSavedConnection(entry: SavedConnection): Promise<SavedConnection[]> {
  const list = await loadSavedConnections();
  const next = list.map((c) => (c.id === entry.id ? entry : c));
  await persistEntry(next, entry);
  return next;
}

export async function removeSavedConnection(id: string): Promise<SavedConnection[]> {
  const list = await loadSavedConnections();
  const next = list.filter((c) => c.id !== id);
  // The credentials go with the connection they belonged to; leaving them behind would mean an
  // entry in the OS store that nothing will ever name again.
  await deleteSecrets(id);
  await persist(next.map((c) => ({ ...c, config: withoutSecrets(c.config) })));
  return next;
}
