import type { ModuleDefinition } from "../module";

/** The modules that lend the tray panel a section, in the order given — the registry's. */
export function traySections(modules: ModuleDefinition[]): ModuleDefinition[] {
  return modules.filter((m) => m.TraySection !== undefined);
}
