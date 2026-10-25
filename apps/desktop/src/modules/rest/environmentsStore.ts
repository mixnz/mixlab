import { createStore, useStore } from "../../core/jsonStore";
import { Store } from "@tauri-apps/plugin-store";
import { envSecretsDelete, envSecretsLoad, envSecretsSave } from "./api";
import {
  addEnvironment,
  findEnvironment,
  newEnvironment,
  removeEnvironment,
  secretsOf,
  updateEnvironment,
  withSecrets,
  withVariables,
  withoutSecrets,
  type Environment,
} from "./environments";

/**
 * The environment list, shared by every REST tab.
 *
 * One thing on disk is one thing in memory, exactly as `requestsStore` does it — an environment
 * edited in the dialog is the same environment the tab behind it resolves against.
 *
 * What differs is where it is written. The names, the `secret` flags and the ordinary values go to
 * `rest-environments.json`; the values marked secret go to the OS credential store, one entry per
 * environment. That is the split `savedConnections.ts` makes for a connection's password, and it
 * is what keeps the file worth reading: it says what a Dev environment is made of without saying
 * what the token is.
 */

const FILE = "rest-environments.json";
const KEY = "environments";

/** The credential-store id an environment's secrets live under. */
const SECRET_PREFIX = "rest-env:";

/** The credential-store id an environment's secret values live under. */
export function secretIdOf(envId: string): string {
  return `${SECRET_PREFIX}${envId}`;
}

/**
 * How long a change waits before it is written.
 *
 * Requests are persisted on every keystroke and that costs one JSON file. The credential store is
 * not free in the same way — on macOS a write may put a prompt on screen — and a token is typed a
 * character at a time. So writing is put off until the typing stops, and the dialog flushes on its
 * way out so the last character is never the one left behind.
 */
const PERSIST_DELAY = 400;

let storePromise: Promise<Store> | null = null;

function getStore(): Promise<Store> {
  if (!storePromise) storePromise = Store.load(FILE);
  return storePromise;
}

let timer: number | null = null;

/** What was last written to the credential store for each environment. An environment whose
 *  secrets have not moved is not written again, so renaming one, or typing in an ordinary value,
 *  never reaches the OS store at all. Sorted, so the same set always stamps the same. */
const written = new Map<string, string>();

function stamp(secrets: Record<string, string>): string {
  return JSON.stringify(Object.entries(secrets).sort(([a], [b]) => a.localeCompare(b)));
}

/** The file, with the credential store's half put back into each environment. */
async function load(): Promise<Environment[]> {
  const store = await getStore();
  const stored = (await store.get<Environment[]>(KEY)) ?? [];
  return Promise.all(
    stored.map(async (env) => {
      // An environment whose entry is gone from the OS store, or one that never had a secret in
      // it, reads as empty rather than as a failure — the same answer `secrets.rs` gives itself.
      const secrets = await envSecretsLoad(secretIdOf(env.id)).catch(
        (): Record<string, string> => ({}),
      );
      const filled = withSecrets(env, secrets);
      // Stamped from the filled environment rather than from what came back, so the first write
      // after a load compares like with like.
      written.set(env.id, stamp(secretsOf(filled)));
      return filled;
    }),
  );
}

/* No `persist` on the store: writing here is debounced *and* half of it goes to the OS credential
   store, so `schedule` below owns it. The mechanics — read once, replace wholesale — are
   `core/jsonStore.ts`. */
const shared = createStore<Environment[]>({ defaults: [], load });

async function persist(): Promise<void> {
  const list = shared.get();
  const store = await getStore();
  await store.set(KEY, list.map(withoutSecrets));
  await store.save();
  for (const env of list) {
    const secrets = secretsOf(env);
    const mark = stamp(secrets);
    if (written.get(env.id) === mark) continue;
    await envSecretsSave(secretIdOf(env.id), secrets);
    written.set(env.id, mark);
  }
}

function schedule(): void {
  if (timer !== null) window.clearTimeout(timer);
  timer = window.setTimeout(() => {
    timer = null;
    void persist().catch(() => {});
  }, PERSIST_DELAY);
}

/** In memory now, on disk shortly. */
function commit(list: Environment[]): void {
  shared.publish(list);
  schedule();
}

/** Writes whatever is still waiting, now, and rejects when the write fails — what sync waits on,
 *  since it agrees only on what the disk holds. */
export function saveEnvironmentsNow(): Promise<void> {
  if (timer !== null) {
    window.clearTimeout(timer);
    timer = null;
  }
  return persist();
}

/** The same for the dialog on its way out, where a failure has nowhere to go. */
export function flushEnvironments(): Promise<void> {
  return saveEnvironmentsNow().catch(() => {});
}

export function useEnvironments(): Environment[] {
  return useStore(shared);
}

/** A new environment at the end of the list, returned so the dialog can select it. */
export function createEnvironment(name: string): Environment {
  const env = newEnvironment(crypto.randomUUID(), name);
  commit(addEnvironment(shared.get(), env));
  return env;
}

/** An edit to an environment. This is the whole of saving one: there is no Save button here
 *  either, for the same reason there is none for a request. */
export function saveEnvironment(env: Environment): void {
  commit(updateEnvironment(shared.get(), env));
}

export function deleteEnvironment(id: string): void {
  commit(removeEnvironment(shared.get(), id));
  // The secrets go with the environment they belonged to; leaving them behind would mean an entry
  // in the OS store that nothing will ever name again.
  written.delete(id);
  void envSecretsDelete(secretIdOf(id)).catch(() => {});
}

/** The names a blocked send asked for, added to an environment as empty rows. */
export function addVariables(id: string, names: string[]): void {
  const env = findEnvironment(shared.get(), id);
  if (env === null) return;
  commit(updateEnvironment(shared.get(), withVariables(env, names)));
}

/** Resolves once the list has been read, secrets included — see `requestsReady`. */
export function environmentsReady(): Promise<void> {
  return shared.ready();
}

/** The list as it stands, for sync to read. */
export function currentEnvironments(): Environment[] {
  return shared.get();
}

/** Sync's write: the whole list, through the same debounced split every other write takes. */
export function replaceEnvironments(list: Environment[]): void {
  commit(list);
}
