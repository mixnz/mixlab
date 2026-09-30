import type { ServiceId } from "@mixengine/api";

/**
 * Builds a `ServiceId` from the package the user picked and the instance name they typed.
 *
 * **There is no id field.** `ServiceId` documents the part before `@` as "the package this is an
 * instance of" (see the type's doc), so letting the user type the whole string invites them to
 * produce an id whose first half matches no package — the daemon refuses, and that refusal cannot
 * say where they mistyped. The package comes from a `Select`; only the part after `@` is free text.
 *
 * An empty instance gives the bare package name (`caddy`), the same shape `service.list` returns
 * for a package's first instance. No name such as `@main` is chosen on the user's behalf: that is a
 * decision, and `ServiceId` goes into the directory name `logs/services/<id>/`, so changing it
 * later is not cheap.
 */
export function serviceIdFrom(packageName: string, instance: string): ServiceId {
  const suffix = instance.trim();
  return suffix === "" ? packageName : `${packageName}@${suffix}`;
}

/**
 * A home has exactly one front end, so its id carries no `@`.
 *
 * **This is a copy of a rule that lives in the daemon, and MixLab cannot look it up.**
 * `package.list` has no field saying how many instances a package allows (see `PackageSummary`,
 * `PackageRelease`) — the rule lives in the recipe. The daemon has the final word: `service.create`
 * with `caddy@main` is refused with `invalid_argument` and "there is one caddy, so its id carries
 * no `@`", and a package MixEngine adds later that the table below does not know yet is blocked the
 * same way.
 *
 * So the table below **only picks the default value of an input field**; it is not where right and
 * wrong are decided. Guessing wrong in either direction costs only a readable refusal, not a broken
 * service.
 *
 * `true` for unknown names: most of the remaining packages are databases and caches, and a new
 * package whose instance-name field the form silently hides is a package that cannot get a second
 * instance from MixLab.
 */
export function takesInstanceName(packageName: string): boolean {
  return !FRONT_ENDS.has(packageName.toLowerCase());
}

const FRONT_ENDS = new Set(["caddy", "nginx", "apache", "apache2", "httpd"]);

/** The suggested name for the first instance — the very `mariadb@main` `ServiceId`'s doc uses as
 *  its example. */
export const DEFAULT_INSTANCE = "main";
