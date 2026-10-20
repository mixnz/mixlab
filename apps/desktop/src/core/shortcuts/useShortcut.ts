import { useEffect, useRef } from "react";
import { hasCtrlOnly, hasPrimaryModifier } from "../platform";
import { isTextEntry } from "../textEntry";
import { decide, type Press } from "./decide";
import { currentCatalogue, enabledIds, modalDepth, register, run, setCatalogue } from "./store";
import type { ShortcutGroup } from "./types";

/**
 * Answers `id` for as long as `enabled` says this pane is the one being looked at.
 *
 * `enabled` is what keeps a chord unambiguous: every connection tab stays mounted behind the one
 * on show, and each of their grids would otherwise answer the same keystroke together.
 *
 * `handler` is read at the moment the key is pressed rather than when the listener was registered,
 * so it may close over state freely — a pane mid-request checks for that inside it, exactly as its
 * button's `disabled` does.
 *
 * **When two panes claim the same chord, the one that registered last wins** — see `decide`, which
 * orders by registration and not by the catalogue. That order is React's effect order: children
 * before parents, and within a parent the order the elements are written in. So `rest.closeRequest`
 * beats `app.closeTab` on `Ctrl+W` because the REST pane is a child of the shell, not because
 * anything says so.
 *
 * Which is worth knowing before relying on it. Two shortcuts that share a chord and are enabled
 * together are a clash — DEV logs one — and the tie is broken by where the components happen to
 * sit. Moving a `useShortcut` call above another in the same component silently changes which one
 * runs. If a pair ever needs an order that does not follow from the tree, the answer is for the
 * outer one to turn its `enabled` off while the inner one is up, which states the intent instead of
 * inheriting it.
 */
export function useShortcut(id: string, handler: () => void, enabled: boolean): void {
  // Through a ref so the registration is made once per spell of being on screen, rather than torn
  // down and remade on every render that hands the hook a fresh closure.
  const latest = useRef(handler);
  latest.current = handler;

  useEffect(() => {
    if (!enabled) return;
    return register({ id, handler: () => latest.current() });
  }, [id, enabled]);
}

/** One entry of {@link useShortcuts}: what `useShortcut` takes, as a value. */
export interface ShortcutEntry {
  id: string;
  handler: () => void;
  enabled: boolean;
}

/** The ids of `entries` that are on, in order, as one string — what `useShortcuts` registers again on. */
export function enabledKeyOf(entries: ShortcutEntry[]): string {
  return entries
    .filter((entry) => entry.enabled)
    .map((entry) => entry.id)
    .join("\n");
}

/**
 * {@link useShortcut} for a list whose length is fixed for the life of the component — one id per
 * module in the registry, say.
 *
 * One effect for the whole list rather than one hook per entry, so the number of hooks never
 * depends on the list and none has to be called in a loop. The registrations are made in the
 * entries' order and are made again, all of them, whenever the set that is on changes; within the
 * list that order is what breaks a tie between two of its ids, as it is for the hook above. Handlers
 * are read when the key is pressed, as `useShortcut` reads its one.
 */
export function useShortcuts(entries: ShortcutEntry[]): void {
  const latest = useRef(entries);
  latest.current = entries;
  const key = enabledKeyOf(entries);

  useEffect(() => {
    if (key === "") return;
    const disposers = key.split("\n").map((id) =>
      register({ id, handler: () => latest.current.find((entry) => entry.id === id)?.handler() }),
    );
    return () => {
      for (const dispose of disposers) dispose();
    };
  }, [key]);
}

/** A keystroke read the one way the app reads them, so nothing asks the DOM these questions
 *  twice and answers them differently the second time. */
export function pressOf(e: KeyboardEvent): Press {
  return {
    key: e.key.toLowerCase(),
    shift: e.shiftKey,
    alt: e.altKey,
    mod: hasPrimaryModifier(e),
    ctrlOnly: hasCtrlOnly(e),
    typing: isTextEntry(e.target),
  };
}

/**
 * Whether a handler would run for this keystroke — asked *before* the event reaches the dispatcher.
 *
 * Exactly one pane needs this and it is worth saying why. xterm answers `keydown` on its own hidden
 * textarea and calls `stopPropagation` on every `Ctrl`+letter it turns into a control character, so
 * the window listener below is never reached at all: `Ctrl+W` went to the shell and the tab stayed
 * open. A terminal therefore has to ask the question from inside xterm's handler and step aside for
 * the answer.
 *
 * "Run" and not "swallow": a chord nothing answers belongs to the shell, and xterm takes the key
 * off the webview by itself anyway.
 */
export function isClaimed(press: Press): boolean {
  const decision = decide(press, currentCatalogue(), {
    modalDepth: modalDepth(),
    enabled: enabledIds(),
  });
  return decision.do === "run";
}

/**
 * The app's one keydown listener. Called once, by the shell.
 *
 * `groups` must be a stable value — a fresh array each render would rebind the listener each
 * render. The catalogues are module-level constants, which is what makes that true.
 */
export function useShortcutDispatcher(groups: ShortcutGroup[]): void {
  useEffect(() => {
    setCatalogue(groups);
    function onKeyDown(e: KeyboardEvent) {
      // CodeMirror declares `preventDefault: true` on its own keymap and sits on the editor
      // element, so it always answers before anything on the window. An editor with the focus
      // wins, and it says so this way rather than by being negotiated with.
      if (e.defaultPrevented) return;

      const decision = decide(pressOf(e), groups, {
        modalDepth: modalDepth(),
        enabled: enabledIds(),
      });

      if (decision.do === "nothing") return;
      // One `preventDefault`, and not only for tidiness: on a Mac this is what keeps `⌘W` on the
      // tab instead of letting the AppKit menu bar close the window. A handler that forgot it lost
      // its key to the operating system; there is nowhere left to forget it now.
      e.preventDefault();
      if (decision.do === "run") run(decision.id);
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [groups]);
}
