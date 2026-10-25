import {
  applySyncChanges,
  asRecord,
  type SyncableCollection,
  type SyncItem,
} from "../../core/syncCollection";
import { envSecretsDelete, envSecretsLoad, envSecretsSave } from "./api";
import { secretsOf, withSecrets, type Environment, type EnvVar } from "./environments";
import {
  currentEnvironments,
  environmentsReady,
  saveEnvironmentsNow,
  replaceEnvironments,
  secretIdOf,
} from "./environmentsStore";
import { newRequest } from "./requests";
import { currentLists, replaceSaved, requestsReady } from "./requestsStore";
import type { RestRequest } from "./types";

/** An allow-list: `lastUsedAt` changes on every send, and `auth` may hold a credential (D5 of
 *  `docs/specs/2026-09-20-t177-a-copy-only-you-can-read-design.md`). */
export function requestToSync(request: RestRequest): SyncItem {
  const { name, method, url, params, headers, body, origin, createdAt } = request;
  return { id: request.id, data: { name, method, url, params, headers, body, origin, createdAt } };
}

export function requestFromSync(synced: SyncItem, local: RestRequest | undefined): RestRequest | null {
  const data = asRecord(synced.data);
  if (
    !data ||
    typeof data.name !== "string" ||
    typeof data.method !== "string" ||
    typeof data.url !== "string" ||
    !Array.isArray(data.params) ||
    !Array.isArray(data.headers) ||
    !asRecord(data.body)
  ) {
    return null;
  }
  // What never travelled stays this machine's; a request new here gets the module's own empty
  // auth rather than one invented in this file.
  const base = local ?? newRequest(synced.id, 0);
  return { ...base, ...(data as Partial<RestRequest>), id: synced.id, auth: base.auth, lastUsedAt: base.lastUsedAt };
}

/** A secret variable travels as a name and a flag; its value is `rest-env-secrets`'. */
export function environmentToSync(env: Environment): SyncItem {
  return {
    id: env.id,
    data: {
      name: env.name,
      vars: env.vars.map((v) => (v.secret ? { name: v.name, secret: true } : { ...v })),
    },
  };
}

export function environmentFromSync(synced: SyncItem, local: Environment | undefined): Environment | null {
  const data = asRecord(synced.data);
  if (!data || typeof data.name !== "string" || !Array.isArray(data.vars)) return null;
  const kept = new Map((local?.vars ?? []).filter((v) => v.secret).map((v) => [v.name, v.value]));
  const vars: EnvVar[] = [];
  for (const raw of data.vars) {
    const v = asRecord(raw);
    if (!v || typeof v.name !== "string") return null;
    vars.push(
      v.secret === true
        ? { name: v.name, value: kept.get(v.name) ?? "", secret: true }
        : { name: v.name, value: typeof v.value === "string" ? v.value : "", secret: false },
    );
  }
  return { id: synced.id, name: data.name, vars };
}

export const requestsSyncable: SyncableCollection = {
  id: "rest-requests",
  labelKey: "restSync.requests",
  read: async () => {
    await requestsReady();
    return currentLists().saved.map(requestToSync);
  },
  write: async (changes) => {
    await requestsReady();
    const { items, skipped } = applySyncChanges(currentLists().saved, changes, (r) => r.id, requestFromSync);
    await replaceSaved(items);
    return skipped;
  },
};

export const environmentsSyncable: SyncableCollection = {
  id: "rest-environments",
  labelKey: "restSync.environments",
  read: async () => {
    await environmentsReady();
    return currentEnvironments().map(environmentToSync);
  },
  write: async (changes) => {
    await environmentsReady();
    const touched = new Set(changes.upserts.map((item) => item.id));
    const { items: next, skipped } = applySyncChanges(
      currentEnvironments(),
      changes,
      (e) => e.id,
      environmentFromSync,
    );
    // A secret variable that arrives with no value here may have one waiting in the vault (D5);
    // saved empty, it would overwrite that.
    const filled = await Promise.all(
      next.map(async (env) =>
        touched.has(env.id) ? fillSecrets(env, await envSecretsLoad(secretIdOf(env.id)).catch(() => ({}))) : env,
      ),
    );
    replaceEnvironments(filled);
    // The store writes on a timer; what sync is about to agree on must be on disk first.
    await saveEnvironmentsNow();
    return skipped;
  },
};

/** An environment's secret values, as `rest-env-secrets` lends them: nothing for one with none. */
export function environmentSecretsToSync(envs: Environment[]): SyncItem[] {
  return envs.flatMap((env) => {
    const data = Object.fromEntries(Object.entries(secretsOf(env)).filter(([, value]) => value !== ""));
    return Object.keys(data).length === 0 ? [] : [{ id: env.id, data }];
  });
}

/** Another machine's secret values for one environment, strings only. */
export function environmentSecretsFromSync(data: unknown): Record<string, string> | null {
  const record = asRecord(data);
  if (!record) return null;
  return Object.fromEntries(
    Object.entries(record).filter((entry): entry is [string, string] => typeof entry[1] === "string"),
  );
}

/** A secret variable with no value here takes what the vault holds for it; one with a value keeps it. */
export function fillSecrets(env: Environment, waiting: Record<string, string>): Environment {
  return {
    ...env,
    vars: env.vars.map((v) =>
      v.secret && v.value === "" && waiting[v.name] !== undefined ? { ...v, value: waiting[v.name] } : v,
    ),
  };
}

export const environmentSecretsSyncable: SyncableCollection = {
  id: "rest-env-secrets",
  labelKey: "restSync.environmentSecrets",
  belongsTo: "rest-environments",
  read: async () => {
    await environmentsReady();
    return environmentSecretsToSync(currentEnvironments());
  },
  write: async (changes) => {
    await environmentsReady();
    const list = currentEnvironments();
    const here = new Set(list.map((env) => env.id));
    let next = list;
    const skipped: string[] = [];
    for (const synced of changes.upserts) {
      const secrets = environmentSecretsFromSync(synced.data);
      if (!secrets) {
        skipped.push(synced.id);
        continue;
      }
      if (here.has(synced.id)) {
        next = next.map((env) => (env.id === synced.id ? withSecrets(env, secrets) : env));
        continue;
      }
      // Not arrived yet: the values wait in the vault, and the environment fills from it (D5).
      // `read` does not return them until then.
      await envSecretsSave(secretIdOf(synced.id), secrets);
      skipped.push(synced.id);
    }
    for (const id of changes.removed) {
      if (here.has(id)) {
        next = next.map((env) =>
          env.id === id ? { ...env, vars: env.vars.map((v) => (v.secret ? { ...v, value: "" } : v)) } : env,
        );
      } else {
        await envSecretsDelete(secretIdOf(id));
      }
    }
    if (next !== list) {
      replaceEnvironments(next);
      await saveEnvironmentsNow();
    }
    return skipped;
  },
};
