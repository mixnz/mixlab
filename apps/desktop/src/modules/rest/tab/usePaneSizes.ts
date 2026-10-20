import { useEffect, useRef, useState, type RefObject } from "react";
import { clampRatio, clampSize } from "../../../components/Splitter";
import {
  MAX_SIDEBAR_WIDTH,
  MAX_SPLIT_RATIO,
  MIN_SIDEBAR_WIDTH,
  MIN_SPLIT_RATIO,
  setSidebarWidth,
  setSplitRatio,
  type Workspace,
} from "../workspace";

/** What a `Splitter` is handed: where a drag starts, moves and ends. */
export interface SplitterHandlers {
  onDragStart: () => void;
  onDrag: (delta: number) => void;
  onDragEnd: (delta: number) => void;
}

export interface PaneSizes {
  /** The sidebar's width, in pixels. */
  width: number;
  /** The request pane's share of the two panes. */
  ratio: number;
  /** The two panes' row, whose width a ratio is a share of. */
  panesRef: RefObject<HTMLDivElement | null>;
  sidebarSplitter: SplitterHandlers;
  paneSplitter: SplitterHandlers;
}

/** The sidebar's width and the split between the request and the response, dragged here and kept
 *  in the workspace file. */
export function usePaneSizes(workspace: Workspace): PaneSizes {
  const [width, setWidth] = useState(workspace.sidebarWidth);
  const [ratio, setRatio] = useState(workspace.splitRatio);
  const dragFrom = useRef(0);
  const panesRef = useRef<HTMLDivElement>(null);

  // The workspace file is read once, after the first render — so the furniture starts at its
  // defaults and moves to what was saved when it arrives.
  useEffect(() => setWidth(workspace.sidebarWidth), [workspace.sidebarWidth]);
  useEffect(() => setRatio(workspace.splitRatio), [workspace.splitRatio]);

  const sidebarSplitter: SplitterHandlers = {
    onDragStart: () => {
      dragFrom.current = width;
    },
    onDrag: (delta) => setWidth(clampSize(dragFrom.current, delta, MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH)),
    onDragEnd: (delta) =>
      setSidebarWidth(clampSize(dragFrom.current, delta, MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH)),
  };

  const paneSplitter: SplitterHandlers = {
    onDragStart: () => {
      dragFrom.current = ratio;
    },
    onDrag: (delta) =>
      setRatio(
        clampRatio(
          dragFrom.current,
          delta,
          panesRef.current?.clientWidth ?? 0,
          MIN_SPLIT_RATIO,
          MAX_SPLIT_RATIO,
        ),
      ),
    onDragEnd: (delta) =>
      setSplitRatio(
        clampRatio(
          dragFrom.current,
          delta,
          panesRef.current?.clientWidth ?? 0,
          MIN_SPLIT_RATIO,
          MAX_SPLIT_RATIO,
        ),
      ),
  };

  return { width, ratio, panesRef, sidebarSplitter, paneSplitter };
}
