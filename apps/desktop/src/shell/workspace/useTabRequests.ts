import { useEffect, useEffectEvent, type Dispatch, type SetStateAction } from "react";
import { onTabRequest, takeTabRequests, type TabRequest } from "../launch";
import { withModule } from "../profiles";
import { MODULES } from "../registry";
import type { TabInfo } from "../tabs";

interface TabRequestsOptions {
  /** The module ids this window draws — `shell/profiles.ts`. */
  enabled: string[];
  onEnabledChange: (enabled: string[]) => void;
  openTab: (moduleId?: string, state?: unknown) => TabInfo;
  setEnabledFor: Dispatch<SetStateAction<Record<string, string>>>;
}

/** Tabs the backend asks for — see `shell/launch.ts` for what they are and why they are drained
 *  rather than delivered. */
export function useTabRequests({ enabled, onEnabledChange, openTab, setEnabledFor }: TabRequestsOptions): void {
  /* What a batch of tab requests does once it has been taken. An effect event, so the drain below
     can listen once and still read `enabled` as it is when the batch arrives, and call the
     `openTab` of that render. */
  const openRequested = useEffectEvent((requests: TabRequest[], ids: string[]) => {
    /* A module this window is not drawing is turned on for the tab that arrived.
       One union across the whole batch and one call: two requests for two hidden modules would
       otherwise each compute their next set from the same stale `enabled`, and the second would
       undo the first.

       Before the tabs are opened, and in the same synchronous block: `enabled` is `App`'s state
       and `tabs` is this component's, and React batches every update made in one tick — after an
       `await` included — into one commit, so `visibleIds` and `tabs` reach the visibility effect
       together and the new tab is never a tab of a hidden module. */
    const before = enabled;
    let next = before;
    for (const request of requests) next = withModule(next, request.moduleId, ids);
    if (next !== before) onEnabledChange(next);

    for (const request of requests) {
      const tab = openTab(request.moduleId, request.state);
      // Hidden when the request arrived — which is the whole of what the notice says.
      if (!before.includes(request.moduleId)) {
        setEnabledFor((prev) => ({ ...prev, [tab.id]: request.moduleId }));
      }
    }
  });

  /* No "cancelled" guard around the drain: what has been taken from the backend's queue is gone
     from it, and a request dropped because StrictMode remounted the workspace between the call and
     its answer would be a tab that never opens. */
  useEffect(() => {
    /* All five ids, not the visible ones: what has been taken from the backend's queue is gone from
       it, and filtering here would leave a `mixlab://` handoff for a hidden module rotting in a queue
       nothing ever drains again. Such a tab opens and draws — `MODULES` is unchanged, so
       `moduleById` finds it — and its module is turned on by `openRequested` so that the visibility
       effect in `useTabs` does not drop it on the same commit. */
    const ids = MODULES.map((m) => m.id);
    async function drain() {
      const requests = await takeTabRequests(ids).catch(() => []);
      if (requests.length === 0) return;
      openRequested(requests, ids);
    }
    const unlisten = onTabRequest(() => void drain());
    void drain();
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);
}
