import type { ProjectPin, RuntimeKind, RuntimeList, RuntimeSummary } from "@mixengine/api";

/**
 * The runtimes one page sees installed, and the pins its projects hold — what the pin-runtime clip
 * changes.
 *
 * Pure, so it is tested in node, and alive as long as the page, like `siteRegistry.ts`. A pin is
 * resolved here the way `core::resolve` resolves a prefix constraint: the newest installed version
 * whose segments agree with every segment written. Carets are not needed by any clip and are not
 * resolved.
 */
export interface PinRegistry {
  installed(kind?: RuntimeKind | null): RuntimeList;
  pins(project: string): ProjectPin[];
  /** Replaces every pin `project` held, as `project.update` does. */
  replace(project: string, pins: Partial<Record<RuntimeKind, string>>): void;
}

function segments(version: string): number[] {
  return version.split(".").map((part) => Number.parseInt(part, 10));
}

function matches(constraint: string, version: string): boolean {
  const wanted = segments(constraint);
  const have = segments(version);
  return wanted.every((part, index) => (have[index] ?? 0) === part);
}

function newestFirst(left: RuntimeSummary, right: RuntimeSummary): number {
  const a = segments(left.version);
  const b = segments(right.version);
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const diff = (b[i] ?? 0) - (a[i] ?? 0);
    if (diff !== 0) return diff;
  }
  return 0;
}

export function createPinRegistry(runtimes: RuntimeSummary[]): PinRegistry {
  const held = new Map<string, Partial<Record<RuntimeKind, string>>>();

  function resolve(kind: RuntimeKind, constraint: string): string | null {
    const found = runtimes
      .filter((runtime) => runtime.kind === kind && matches(constraint, runtime.version))
      .sort(newestFirst);
    return found[0]?.version ?? null;
  }

  return {
    installed(kind) {
      return { runtimes: runtimes.filter((runtime) => !kind || runtime.kind === kind) };
    },

    pins(project) {
      const pins = held.get(project) ?? {};
      return (Object.entries(pins) as [RuntimeKind, string][]).map(([kind, constraint]) => {
        const resolved = resolve(kind, constraint);
        return {
          kind,
          constraint,
          source: { from: "registered" },
          resolved,
          hint: resolved === null ? "mix runtime available" : null,
        };
      });
    },

    replace(project, pins) {
      held.set(project, { ...pins });
    },
  };
}
