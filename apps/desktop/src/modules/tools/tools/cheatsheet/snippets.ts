/**
 * Cheatsheet snippets: a command with blanks, and the way to fill those blanks.
 *
 * Filling in parameters is the **feature**, not a container — that is what sets this tool apart
 * from a notes file, and why it passes the module's tool admission criterion.
 */

export interface Snippet {
  id: string;
  title: string;
  /** The group to sort the list by: `mysql`, `postgres`, `docker`, `ssh`… A free string. */
  group: string;
  /** The command, with parameters written as `{{name}}`. */
  template: string;
}

const PARAM = /\{\{([A-Za-z0-9_]+)\}\}/g;

/** The parameter names, in order of first appearance, without repeats. */
export function paramsOf(template: string): string[] {
  const names: string[] = [];
  for (const match of template.matchAll(PARAM)) {
    const name = match[1]!;
    if (!names.includes(name)) names.push(name);
  }
  return names;
}

/**
 * Replaces `{{name}}` with values.
 *
 * A name with no value — or with an empty value — **keeps `{{name}}` as is**: an unfilled slot has
 * to be visible in the output, rather than vanishing into whitespace and letting the user copy a
 * command missing an argument.
 *
 * **No quotes are added.** Not every parameter stands in a shell argument position, and wrapping
 * where the template already wraps breaks in a far harder-to-see way than it breaks now.
 */
export function fill(template: string, values: Record<string, string>): string {
  return template.replace(PARAM, (whole, name: string) => {
    const value = values[name];
    return value === undefined || value === "" ? whole : value;
  });
}

/** A snippet being edited: everything except `id`, which only the list can assign. */
export type SnippetDraft = Omit<Snippet, "id">;

/**
 * Ids that do not collide, without needing `crypto.randomUUID`.
 *
 * This list belongs to one person on one machine and is a few dozen entries long; the only thing an
 * id has to do is tell two entries added back to back apart, and a counter running after a
 * timestamp does exactly that.
 */
let counter = 0;
function nextId(): string {
  counter += 1;
  return `s${Date.now().toString(36)}${counter.toString(36)}`;
}

export function addSnippet(list: Snippet[], draft: SnippetDraft): Snippet[] {
  return [...list, { ...draft, id: nextId() }];
}

export function updateSnippet(list: Snippet[], id: string, draft: SnippetDraft): Snippet[] {
  return list.map((snippet) => (snippet.id === id ? { ...draft, id } : snippet));
}

export function removeSnippet(list: Snippet[], id: string): Snippet[] {
  return list.filter((snippet) => snippet.id !== id);
}

function isSnippet(value: unknown): value is Snippet {
  if (typeof value !== "object" || value === null) return false;
  const it = value as Record<string, unknown>;
  return (
    typeof it.id === "string" &&
    it.id !== "" &&
    typeof it.title === "string" &&
    typeof it.group === "string" &&
    typeof it.template === "string"
  );
}

/**
 * Reads what was taken from disk into a snippet list.
 *
 * Checks the shape and only the shape, as `parseToolsTabState` does — and lives here rather than in
 * `snippetsStore.ts` because this is the pure part, i.e. the part that can be tested. A file
 * written by an old version, or a hand-edited file, loses the broken entries rather than breaking
 * the whole tool.
 */
export function readSnippets(value: unknown): Snippet[] {
  return Array.isArray(value) ? value.filter(isSnippet) : [];
}
