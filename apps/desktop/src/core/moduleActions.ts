/**
 * What one module can ask of another without importing it.
 *
 * Modules never import each other (`npm run lint`), and the launch queue only opens tabs. Some
 * requests are neither: the MixEngine module's *Save as Terminal target* wants a target saved, now,
 * with an answer it can show, and no tab opened over what the person is reading. So a module lends
 * named actions in its `ModuleDefinition`, the registry hands them here, and another module calls
 * one by the lender's id. The payload is the lender's to validate, exactly as a restored tab's state
 * is; nothing here knows what it means.
 */

/** One action a module lends. It validates its own payload. */
export type ModuleAction = (payload: unknown) => Promise<unknown>;

const lent = new Map<string, ModuleAction>();

/** The registry's half: what `moduleId` lends, by name. */
export function provideModuleActions(moduleId: string, actions: Record<string, ModuleAction>): void {
  for (const [name, action] of Object.entries(actions)) lent.set(`${moduleId}.${name}`, action);
}

/** Calls `name` on `moduleId`. Rejects when that module lends no such action. */
export async function callModuleAction(moduleId: string, name: string, payload: unknown): Promise<unknown> {
  const action = lent.get(`${moduleId}.${name}`);
  if (action === undefined) throw new Error(`the ${moduleId} module lends no action named ${name}`);
  return action(payload);
}
