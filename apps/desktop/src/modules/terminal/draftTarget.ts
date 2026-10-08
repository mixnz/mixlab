import { localShells } from "./api";
import { addTarget, currentTargets } from "./savedTargetsStore";
import { loadTerminalSettings } from "./settingsStore";
import type { OnRestore, SavedLocalTarget, SavedTarget } from "./types";

/**
 * A target another module asks this one to save — T205. The MixEngine module's *Save as Terminal
 * target* sends one for a project's dev server, and it is saved there and then: the person pressed
 * Save, so a target in the list is what they expect next time, with no tab opened over what they
 * were reading.
 */
export interface DraftTarget {
  name: string;
  /** `""` is the machine's default shell: the sender cannot know this machine's shell names. */
  shellName: string;
  cwd: string;
  env: Record<string, string>;
  pathPrepend: string[];
  runOnConnect: string;
  onRestore: OnRestore;
}

const ON_RESTORE: readonly OnRestore[] = ["run", "type", "none"];

/** The payload, every field checked: it comes from another module, so nothing is trusted. */
export function parseDraft(value: unknown): DraftTarget | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const draft = value as Record<string, unknown>;
  if (typeof draft.name !== "string" || draft.name === "") return null;
  if (typeof draft.cwd !== "string" || draft.cwd === "") return null;
  if (typeof draft.shellName !== "string" || typeof draft.runOnConnect !== "string") return null;
  const env = draft.env;
  if (typeof env !== "object" || env === null || Array.isArray(env)) return null;
  if (!Object.values(env).every((item) => typeof item === "string")) return null;
  const pathPrepend = draft.pathPrepend;
  if (!Array.isArray(pathPrepend) || !pathPrepend.every((item) => typeof item === "string")) return null;
  // An unknown word is the cautious one: typed and left for Enter.
  const onRestore = ON_RESTORE.find((word) => word === draft.onRestore) ?? "type";
  return {
    name: draft.name,
    shellName: draft.shellName,
    cwd: draft.cwd,
    env: env as Record<string, string>,
    pathPrepend: pathPrepend as string[],
    runOnConnect: draft.runOnConnect,
    onRestore,
  };
}

/** The saved entry a draft becomes, on `shellName`. Empty fields are left out, and `run`, which is
 *  what an absent `onRestore` means — the same rule the form writes by. */
export function savedFromDraft(draft: DraftTarget, shellName: string, id: string): SavedLocalTarget {
  return {
    id,
    name: draft.name,
    kind: "local",
    shellName,
    cwd: draft.cwd,
    runOnConnect: draft.runOnConnect,
    ...(draft.onRestore === "run" ? {} : { onRestore: draft.onRestore }),
    ...(Object.keys(draft.env).length === 0 ? {} : { env: draft.env }),
    ...(draft.pathPrepend.length === 0 ? {} : { pathPrepend: draft.pathPrepend }),
  };
}

/** An entry already saved for the same thing — same name, folder and command — so a second press
 *  of Save does not make a second row. */
export function sameTarget(targets: SavedTarget[], candidate: SavedLocalTarget): SavedTarget | undefined {
  return targets.find(
    (target) =>
      target.kind === "local" &&
      target.name === candidate.name &&
      target.cwd === candidate.cwd &&
      target.runOnConnect === candidate.runOnConnect,
  );
}

/**
 * The `saveTarget` action this module lends (`core/moduleActions.ts`): saves the draft, or finds
 * the entry already saved for it. Resolves to that entry's id.
 */
export async function saveDraftTarget(payload: unknown): Promise<{ id: string }> {
  const draft = parseDraft(payload);
  if (draft === null) throw new Error("not a target this module can save");

  // `""` takes the shell a new tab would: the one Settings names, else the first detected.
  let shellName = draft.shellName;
  if (shellName === "") {
    const [shells, settings] = await Promise.all([localShells(), loadTerminalSettings()]);
    const preferred = shells.find((shell) => shell.name === settings.defaultShell) ?? shells[0];
    if (preferred === undefined) throw new Error("no shell was found on this machine");
    shellName = preferred.name;
  }

  const entry = savedFromDraft(draft, shellName, crypto.randomUUID());
  const existing = sameTarget(await currentTargets(), entry);
  if (existing !== undefined) return { id: existing.id };
  await addTarget(entry);
  return { id: entry.id };
}
