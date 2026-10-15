import { useEffect, useEffectEvent, useRef, useState } from "react";
import { useTranslation } from "../../../i18n";
import { takeHandoff } from "../api";
import { arrivesConnected } from "./handoffArrival";
import { reconnectsOnRestore } from "../saved/savedConnections";
import { parseDbTabState } from "../tabState";
import type { SavedConnection } from "../types";
import type { Connection } from "./useConnection";
import type { ConnectionFormState } from "./useConnectionForm";

interface Options {
  /** The tab's slot from the last session, as the shell hands it down. */
  restored: unknown;
  savedConnections: SavedConnection[];
  savedConnectionsLoaded: boolean;
  editor: ConnectionFormState;
  connection: Connection;
  openAndConnect: (entry: SavedConnection) => void;
  onTitleChange: (title: string) => void;
  onStateChange: (state: unknown) => void;
}

/** How the tab arrives: back at what it had open last launch, or holding a connection another
 *  program handed over. Each happens once per tab, and only one of them. */
export function useTabArrival({
  restored,
  savedConnections,
  savedConnectionsLoaded,
  editor,
  connection,
  openAndConnect,
  onTitleChange,
  onStateChange,
}: Options) {
  const { t } = useTranslation();
  /* What this tab was connected to last launch, taken once. A snapshot and not a live read: the
     moment this tab connects it writes a new value, and reading that back would be the tab
     restoring itself from itself. */
  const [restoredState] = useState(() => parseDbTabState(restored));
  /** Whether the restore below has had its one turn — win or lose. Without it, another tab saving
   *  a connection publishes a new list, the effect runs again, and this tab opens a second
   *  connection to the same server. */
  const restoreTried = useRef(false);

  /* The tab coming back to what it had open, once, the first time it is looked at — which for a
     tab restored from the last session is the first time it is mounted at all.
     `savedConnectionsLoaded` is the whole reason this waits: the list is empty until the file has
     been read, and acting on it before then would read "the connection was deleted" every launch.
     A connection that really has gone leaves the form as it opens, with no banner — nothing failed.

     A tab that was in its workspace dials again; one the user had disconnected only loads the
     connection into the form and marks it in the sidebar. Both come back looking like what was
     left behind, which is the whole of the promise — the second half of it used to be missing, and
     that tab opened blank under a title naming a connection it no longer pointed at.

     `openAndConnect` and `editor.applySaved` are rebuilt every render, so they are reached
     through an effect event rather than listed.

     A tab whose password is MixEngine's comes back as its form even if it was connected: dialling
     it would be a second Keychain question before anybody asked to connect. */
  const restoreEntry = useEffectEvent((entry: SavedConnection, connected: boolean) => {
    if (connected && reconnectsOnRestore(entry)) openAndConnect(entry);
    else editor.applySaved(entry);
  });
  useEffect(() => {
    if (restoreTried.current || restoredState === null || !("savedId" in restoredState) || !savedConnectionsLoaded) return;
    restoreTried.current = true;
    const entry = savedConnections.find((c) => c.id === restoredState.savedId);
    if (entry === undefined) return;
    restoreEntry(entry, restoredState.connected);
  }, [restoredState, savedConnectionsLoaded, savedConnections]);

  /* A tab opened for a connection another program handed over. Once, like the restore above and
     through the same ref; unlike it, nothing on disk is waited for — what is taken is in the
     backend's memory, and it is taken now or not at all.

     The slot is forgotten first: the id means nothing after this call, and the session must not
     carry a pointer to nowhere. What is left once connected is a tab named after the launcher's
     label, pointing at no saved connection, with the form holding everything — the password too, so
     a server that was not up yet is a banner and a Connect button, not a form to fill in again.
     A take that fails (an id from an old session, a second tab for the same id) is an empty form
     and no banner: nothing the user did has gone wrong.

     Whether it is dialled at once is `arrivesConnected`: a handoff from MixEngine brought the
     password and is; a `mixlab://` link from a browser brought everything but, and is shown as a
     form with the caret where the one missing thing goes, rather than dialled to a certain
     "access denied". */
  useEffect(() => {
    if (restoreTried.current || restoredState === null || !("handoffId" in restoredState)) return;
    restoreTried.current = true;
    onStateChange(undefined);
    takeHandoff(restoredState.handoffId).then(
      (handoff) => {
        if (connection.isClosed()) return;
        editor.fill(handoff.config, handoff.keyring_ref ?? null, handoff.label);
        onTitleChange(handoff.label);
        if (arrivesConnected(handoff.config)) {
          void connection.connect({
            config: handoff.config,
            keyringRef: null,
            savedId: editor.editingId,
            title: handoff.label,
            name: editor.saveAsName,
          });
        } else {
          connection.setStatus(t("connection.handoffNeedsPassword"));
          editor.askForPassword();
        }
      },
      () => {},
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps -- runs once per tab, on the snapshot taken at mount; `connect` and the two callbacks are rebuilt every render.
  }, [restoredState]);
}
