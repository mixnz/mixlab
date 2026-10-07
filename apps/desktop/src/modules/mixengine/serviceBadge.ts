/**
 * What a service's badge is drawn from, read off its id alone (`<package>@<instance>`).
 *
 * The letters and the hue come from the package, so every `php-fpm@*` wears the same badge; the
 * instance is pinned to the corner only when another listed service shares the package, which is
 * when the badge alone would not tell them apart. A version instance is cut to `major.minor`: the
 * corner has room for `8.4`, not `8.4.26`. Still a hash of a name, not a table of what a
 * package is (ADR 0026).
 */
export interface ServiceBadge {
  name: string;
  tag: string | undefined;
}

function split(id: string): [string, string] {
  const at = id.indexOf("@");
  return at <= 0 ? [id, ""] : [id.slice(0, at), id.slice(at + 1)];
}

/** `8.4.26` -> `8.4`; anything that does not start with `major.minor` is kept whole. */
function shortInstance(instance: string): string {
  return /^\d+\.\d+/.exec(instance)?.[0] ?? instance;
}

export function serviceBadge(id: string, allIds: readonly string[]): ServiceBadge {
  const [pkg, instance] = split(id);
  if (instance === "") return { name: pkg, tag: undefined };
  const siblings = allIds.filter((other) => split(other)[0] === pkg).length;
  return { name: pkg, tag: siblings > 1 ? shortInstance(instance) : undefined };
}
