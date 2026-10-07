/**
 * What a service's badge is drawn from, read off its id alone (`<package>@<instance>`).
 *
 * The letters and the hue come from the package, so every `php-fpm@*` wears the same badge; the
 * instance is pinned to the corner only when another listed service shares the package, which is
 * when the badge alone would not tell them apart. A version instance is cut to `major.minor`: the
 * corner has room for `8.4`, not `8.4.26`. **Only a version is pinned** (T200): an instance named
 * for something else — `php-fpm@phpmyadmin`, `mariadb@main` — overflowed the corner, and the row
 * beside the badge already says its whole name. Still a hash of a name, not a table of what a
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

/** `8.4.26` -> `8.4`, `8` -> `8`; `undefined` for an instance that is not a version. */
function versionTag(instance: string): string | undefined {
  return /^\d+(\.\d+)?/.exec(instance)?.[0];
}

export function serviceBadge(id: string, allIds: readonly string[]): ServiceBadge {
  const [pkg, instance] = split(id);
  if (instance === "") return { name: pkg, tag: undefined };
  const siblings = allIds.filter((other) => split(other)[0] === pkg).length;
  return { name: pkg, tag: siblings > 1 ? versionTag(instance) : undefined };
}
