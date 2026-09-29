import type { CategoryUsage } from "@mixengine/api";
import type { DiskCategory } from "@mixengine/api";

/** The `CleanupQuery` field name for a category, or `null` for the three categories that cannot be
 *  cleaned through `daemon.cleanup` (`runtimes`/`data`/`certs`) — only `logs`/`cache` ever have
 *  `reclaim: "by_cleanup"` (`Reclaim` doc comment: "Only Logs and Cache ever appear"). */
export function cleanupFlagFor(id: DiskCategory): "keep_logs" | "keep_cache" | null {
  if (id === "logs") return "keep_logs";
  if (id === "cache") return "keep_cache";
  return null;
}

/** Whether this category has a clean-up button through `daemon.cleanup` — the daemon says so; it
 *  is not inferred from the category name. */
export function isCleanupReclaimable(category: CategoryUsage): boolean {
  return category.reclaim.reclaim === "by_cleanup";
}
