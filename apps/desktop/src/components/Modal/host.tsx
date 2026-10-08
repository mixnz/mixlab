import { createContext, useContext, useState, type ReactNode } from "react";
import styles from "./host.module.css";

/**
 * Where a tab's dialogs are drawn — over that tab's pane and nothing else.
 *
 * A dialog belongs to what opened it. One opened inside a tab covers that tab, hides with it when
 * another tab comes to the front, and is still there on the way back; the tab strip above stays
 * live, so a dialog never holds the whole window for one module's question. Outside any tab — the
 * Settings dialog, the tray — there is no host, and a dialog covers the window as before.
 *
 * `null` is "not inside a tab". `{ element: null }` is "inside a tab whose host has not mounted
 * yet": a dialog waits that one commit rather than opening over the window and then moving.
 */
const DialogHostContext = createContext<{ element: HTMLElement | null } | null>(null);

/** The host this dialog is drawn into, as `DialogHostContext` says. */
export function useDialogHost(): { element: HTMLElement | null } | null {
  return useContext(DialogHostContext);
}

/** Whether a host's tab is the one on screen: a pane out of sight is `display: none`, and an
 *  element under one has no offset parent. */
export function hostShown(element: HTMLElement): boolean {
  return element.offsetParent !== null;
}

/** Wraps one tab's pane, and puts the layer its dialogs are drawn into over it. */
export function DialogHost({ children }: { children: ReactNode }) {
  const [element, setElement] = useState<HTMLElement | null>(null);
  return (
    <DialogHostContext.Provider value={{ element }}>
      {children}
      <div ref={setElement} className={styles.layer} />
    </DialogHostContext.Provider>
  );
}
