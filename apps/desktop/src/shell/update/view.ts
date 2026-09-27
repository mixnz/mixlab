/** What the Updates pane and the corner panel draw, decided from values alone: T187 spec D9, T188 spec D1. */

export type PlacementKind = "development" | "swap" | "installer" | "elsewhere";

export type View =
  | "development"
  | "elsewhere"
  | "upToDate"
  | "noBuild"
  | "offer"
  | "skipped"
  | "downloading"
  | "ready"
  | "installing"
  | "handedOver"
  | "finish";

export interface ViewInput {
  current: string;
  placement: { kind: PlacementKind };
  /** The feed's version, and whether it has a build for this machine; null before any read. */
  offered: { version: string; hasBuild: boolean } | null;
  skipped: string | null;
  installing: boolean;
  handedOver: boolean;
  /** What the installer has put on disk, polled while handed over. */
  onDisk: string | null;
  /** A download is running now. */
  downloading: boolean;
  /** The version whose download is on disk and proved; null when none. */
  downloaded: string | null;
}

/** Whether `a` is a later version than `b`, compared as numbers part by part. */
export function isNewer(a: string, b: string): boolean {
  const pa = a.split(".").map(Number);
  const pb = b.split(".").map(Number);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const d = (pa[i] ?? 0) - (pb[i] ?? 0);
    if (d !== 0) return d > 0;
  }
  return false;
}

export function updateView(input: ViewInput): View {
  const { placement, offered } = input;
  if (placement.kind === "development") return "development";
  if (placement.kind === "elsewhere") return "elsewhere";
  if (input.installing) return "installing";
  if (input.downloading) return "downloading";
  if (input.handedOver) {
    return offered && input.onDisk && !isNewer(offered.version, input.onDisk) ? "finish" : "handedOver";
  }
  if (!offered || !isNewer(offered.version, input.current)) return "upToDate";
  if (!offered.hasBuild) return "noBuild";
  if (input.skipped === offered.version) return "skipped";
  return input.downloaded === offered.version ? "ready" : "offer";
}

export type Panel = "hidden" | "offer" | "downloading" | "ready" | "failed";

/**
 * The corner panel: T188 D1. *Later* hides it until the next window start, except while a download
 * somebody started is running; a failed download or install replaces the offer it came from.
 */
export function panelView({ view, later, failed }: { view: View; later: boolean; failed: boolean }): Panel {
  if (view === "downloading") return "downloading";
  if (later) return "hidden";
  if (view !== "offer" && view !== "ready") return "hidden";
  return failed ? "failed" : view;
}
