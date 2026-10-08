/**
 * Which project the Projects screen opens on, when another screen sends someone there — T205.
 *
 * `sitesNavigation.ts`'s shape, the other way round: the Sites screen's *What to run* and an apply
 * that has just finished both want the person to land on one project with its row already open,
 * since that row is where the project's runtimes, sites and steps are. Taken once, so a later
 * visit to the screen opens nothing by itself.
 */
let pendingProject: string | null = null;

export function requestProjectDetail(project: string): void {
  pendingProject = project;
}

export function takePendingProjectDetail(): string | null {
  const project = pendingProject;
  pendingProject = null;
  return project;
}
