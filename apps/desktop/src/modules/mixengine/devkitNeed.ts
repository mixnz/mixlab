import type { BlueprintPlan, PackageRelease, ProjectPin, RuntimeRelease } from "@mixengine/api";

/**
 * The same question asked of a plan, before anything is installed: the Ruby line the blueprint
 * pins (`register_project`'s `pins.ruby`), whether any release of that line lacks `native gems`,
 * and whether a devkit is installed. Asked here so the Apply dialog can offer the devkit with the
 * apply, where the decision is made, rather than after it has left a broken project.
 */
export function planDevkitNeed(
  plan: BlueprintPlan,
  releases: RuntimeRelease[],
  devkit: { release: PackageRelease | null; installed: boolean },
): { offer: PackageRelease | null } | null {
  if (devkit.installed) return null;
  let line: string | undefined;
  for (const step of plan.steps) {
    if (step.action.action === "register_project") line = step.action.pins?.ruby;
  }
  if (line === undefined) return null;
  const inLine = releases.filter(
    (release) =>
      release.kind === "ruby" && (release.version === line || release.version.startsWith(`${line}.`)),
  );
  if (!inLine.some((release) => release.lacks?.["native gems"] !== undefined)) return null;
  return { offer: devkit.release };
}

/**
 * Whether a project's steps would meet a Ruby that cannot build gems with C extensions — T206a,
 * surfaced where the steps are (T205).
 *
 * Measured by hand: `rails new` on a Windows Ruby with no devkit ran `bundle install`, which stopped
 * at the first gem with a C extension, and `rails server` then listed sixty missing gems. A note
 * under the step said to install the devkit first, and a note is what nobody reads before pressing
 * Run. So the panel asks the question itself: the Ruby this project resolves to, whether its release
 * lacks `native gems` (the index's `lacks`, T206), and whether a devkit is installed.
 *
 * `null` when nothing stands in the way; otherwise the devkit to offer, `null` where this machine is
 * offered none.
 */
export function devkitNeed(
  pins: ProjectPin[],
  releases: RuntimeRelease[],
  devkit: { release: PackageRelease | null; installed: boolean },
): { offer: PackageRelease | null } | null {
  if (devkit.installed) return null;
  const resolved = pins.find((pin) => pin.kind === "ruby")?.resolved ?? null;
  if (resolved === null) return null;
  const release = releases.find((candidate) => candidate.kind === "ruby" && candidate.version === resolved);
  if (release?.lacks?.["native gems"] === undefined) return null;
  return { offer: devkit.release };
}
