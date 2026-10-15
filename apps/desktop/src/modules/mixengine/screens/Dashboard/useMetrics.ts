import { useEffect, useState } from "react";
import type { MetricsFrame } from "@mixengine/api";
import { serialQueue } from "../../../../core/serialQueue";
import { useWindowFocused } from "../../../../core/windowFocus";
import * as api from "../../api";
import { parseMetricsFrame } from "../../metricsState";

/** The latest `/metrics` frame, held open while the Dashboard is looked at. */
export function useMetrics(active: boolean): MetricsFrame | null {
  /** The latest `/metrics` frame, or `null` when there is none yet (the stream is not open, or no
   *  frame has arrived). */
  const [frame, setFrame] = useState<MetricsFrame | null>(null);
  const focused = useWindowFocused();

  /**
   * `/metrics` ties its lifetime to `active`, not to mount/unmount like `/events`.
   *
   * **Opening this connection is the subscription** — MixEngine samples at 1 Hz while anyone holds
   * the stream, and once a minute when nobody does. `MixEngineTab.tsx` keeps every visited screen
   * in the DOM instead of unmounting on a tab switch, so if this were tied to unmount, leaving the
   * Dashboard for another screen would close nothing — the daemon would be stuck sampling fast
   * forever with nobody looking. The effect cleanup runs in both cases (`active` turning `false`,
   * and a real unmount), so tying it to `active` covers both.
   *
   * **And only while the window has focus**, the tray panel's rule. `active` says Dashboard is the
   * tab in front, not that anybody is looking: a window left on Dashboard behind another
   * application held the stream open for hours and kept the daemon sampling every second.
   *
   * **Opening and closing go through one queue.** Focus can leave and come back while a stream is
   * still connecting, and the window keeps one `/metrics` slot: a close sent beside the next
   * opening could land after it and end the stream the Dashboard now wants. Queued, each close
   * follows the opening it belongs to, and the next opening follows that close.
   */
  const measuring = active && focused;
  const [metricsQueue] = useState(serialQueue);
  useEffect(() => {
    if (!measuring) return;
    let live = true;
    metricsQueue(async () => {
      // Put away before its turn came: nothing to open.
      if (!live) return;
      await api.metricsWatch((raw) => {
        if (!live) return;
        const next = parseMetricsFrame(raw);
        if (next !== null) setFrame(next);
      });
    });
    return () => {
      live = false;
      setFrame(null);
      metricsQueue(() => api.metricsUnwatch());
    };
  }, [measuring, metricsQueue]);

  return frame;
}
