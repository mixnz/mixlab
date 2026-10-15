import { sshFromSync, sshToSync } from "../../../core/ssh";
import {
  applySyncChanges,
  asRecord,
  type SyncableCollection,
  type SyncChanges,
  type SyncItem,
} from "../../../core/syncCollection";
import {
  deleteSecrets,
  loadSavedConnections,
  loadSecrets,
  readSecrets,
  saveSecrets,
  withSecrets,
  withoutSecrets,
} from "../saved/savedConnections";
import { addConnection, removeConnection, updateConnection } from "../saved/savedConnectionsStore";
import type { ConnectionConfig, SavedConnection } from "../types";

/**
 * What travels of a connection: what it *is* (D5), and its read-only guard.
 *
 * An allow-list: a sidebar's width, a pin, a scan ceiling and a MixEngine keyring reference belong
 * to this machine, and every credential belongs to `connection-secrets` (T177f).
 */
export function connectionToSync(connection: SavedConnection): SyncItem {
  const config = withoutSecrets(connection.config);
  if (config.ssh) config.ssh = sshToSync(config.ssh);
  return {
    id: connection.id,
    data: {
      name: connection.name,
      config,
      // A guard, not a preference: a production connection that arrives without it is more
      // dangerous on the second machine than it was on the first.
      readOnly: connection.readOnly === true,
    },
  };
}

/** Another machine's connection, over this machine's: what travelled replaces, the rest stays. */
export function connectionFromSync(
  synced: SyncItem,
  local: SavedConnection | undefined,
): SavedConnection | null {
  const data = asRecord(synced.data);
  const config = asRecord(data?.config);
  if (!data || typeof data.name !== "string" || !config) return null;
  const incoming = { ...(config as unknown as ConnectionConfig) };
  if (incoming.ssh) incoming.ssh = sshFromSync(incoming.ssh, local?.config.ssh);
  return {
    ...local,
    id: synced.id,
    name: data.name,
    // No credential travels (D5). The one this machine has is kept; a new connection has none.
    config: local ? withSecrets(incoming, readSecrets(local.config, local.keyringRef)) : incoming,
    readOnly: data.readOnly === true ? true : undefined,
  };
}

/** Through the store's own add, update and remove, which own the split with the credential store. */
async function write(changes: SyncChanges): Promise<string[]> {
  const current = await loadSavedConnections();
  const had = new Set(current.map((connection) => connection.id));
  const { items: next, skipped } = applySyncChanges(current, changes, (c) => c.id, connectionFromSync);
  const touched = new Set(changes.upserts.map((item) => item.id));

  for (const id of changes.removed) if (had.has(id)) await removeConnection(id);
  for (const connection of next) {
    if (!touched.has(connection.id)) continue;
    if (had.has(connection.id)) {
      await updateConnection(connection);
      continue;
    }
    // Saved with no credential, a new connection would delete the vault entry its secrets may
    // already be waiting in (D5). It takes them first.
    const waiting = await loadSecrets(connection.id);
    await addConnection({ ...connection, config: withSecrets(connection.config, waiting) });
  }
  return skipped;
}

export const connectionsSyncable: SyncableCollection = {
  id: "connections",
  labelKey: "dbSync.connections",
  read: async () => (await loadSavedConnections()).map(connectionToSync),
  write,
};

/** The credentials `connection-secrets` carries (D5). */
const SECRET_KEYS = ["password", "uri", "sshPassword", "sshPassphrase"] as const;

/**
 * A connection's credentials, as `connection-secrets` lends them: what `readSecrets` finds, so
 * never a password MixEngine's keyring holds, and no item at all for a connection with none.
 */
export function connectionSecretsToSync(connections: SavedConnection[]): SyncItem[] {
  return connections.flatMap((connection) => {
    const secrets = readSecrets(connection.config, connection.keyringRef) as Record<string, string | undefined>;
    const data: Record<string, string> = {};
    for (const key of SECRET_KEYS) {
      const value = secrets[key];
      if (value) data[key] = value;
    }
    return Object.keys(data).length === 0 ? [] : [{ id: connection.id, data }];
  });
}

/** Another machine's credentials for one connection: the four fields, strings only. */
export function connectionSecretsFromSync(data: unknown): Record<string, string> | null {
  const record = asRecord(data);
  if (!record) return null;
  const out: Record<string, string> = {};
  for (const key of SECRET_KEYS) {
    const value = record[key];
    if (typeof value === "string" && value !== "") out[key] = value;
  }
  return out;
}

/**
 * Credentials into the vault. A connection this machine has is saved through the store, so its
 * tabs see the new password at once; one that has not arrived yet gets its entry anyway, and finds
 * it when it comes (D5).
 */
async function writeSecrets(changes: SyncChanges): Promise<string[]> {
  const current = new Map((await loadSavedConnections()).map((connection) => [connection.id, connection]));
  const skipped: string[] = [];
  for (const synced of changes.upserts) {
    const secrets = connectionSecretsFromSync(synced.data);
    if (!secrets) {
      skipped.push(synced.id);
      continue;
    }
    const local = current.get(synced.id);
    if (!local) {
      // Parked for a connection still to come: `read` does not return it until then (L4).
      await saveSecrets(synced.id, secrets);
      skipped.push(synced.id);
      continue;
    }
    // A password MixEngine's keyring holds for this connection stays there.
    if (local.keyringRef) delete secrets.password;
    await updateConnection({ ...local, config: withSecrets(withoutSecrets(local.config), secrets) });
  }
  for (const id of changes.removed) {
    const local = current.get(id);
    if (local) await updateConnection({ ...local, config: withoutSecrets(local.config) });
    else await deleteSecrets(id);
  }
  return skipped;
}

export const connectionSecretsSyncable: SyncableCollection = {
  id: "connection-secrets",
  labelKey: "dbSync.connectionSecrets",
  belongsTo: "connections",
  read: async () => connectionSecretsToSync(await loadSavedConnections()),
  write: writeSecrets,
};
