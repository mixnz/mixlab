import { invoke } from "@tauri-apps/api/core";

/**
 * The OS credential store: Windows Credential Manager, the macOS Keychain, the Secret Service on
 * Linux. `src-tauri/src/secrets.rs`.
 *
 * Every module keeps its credentials here under an id of its own choosing — a saved connection's
 * id, a terminal host's, `rest-env:<envId>` — as one flat map of strings per entry. What goes in
 * the map is the module's business; this file only carries it. Saving an empty map deletes the
 * entry rather than storing `{}`.
 */

export function saveSecrets<T extends object>(id: string, secrets: T): Promise<void> {
  return invoke<void>("secrets_save", { id, secrets });
}

/** What is stored under `id`, or an empty map when nothing ever was. */
export function loadSecrets<T extends object>(id: string): Promise<T> {
  return invoke<T>("secrets_load", { id });
}

export function deleteSecrets(id: string): Promise<void> {
  return invoke<void>("secrets_delete", { id });
}
