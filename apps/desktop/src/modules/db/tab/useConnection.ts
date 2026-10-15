import { useCallback, useEffect, useRef, useState } from "react";
import { errorMessage } from "../../../core/errors";
import { fileName } from "../../../core/paths";
import { useTranslation } from "../../../i18n";
import { connectDb, disconnectDb, testSshTunnel } from "../api";
import { kindLabel } from "../connectionForm";
import { withResolvedPassword } from "../savedConnections";
import type { ConnectionConfig, SavedConnection } from "../types";
import { mongoUriHost } from "./mongoUri";

/**
 * What the tunnel test is currently saying. The tone travels beside the sentence rather than being
 * read back out of it: the three messages are translated, so the text itself can't be matched on.
 */
export interface TunnelStatus {
  tone: "pending" | "ok" | "error";
  message: string;
}

/** What `connect` dials, and what the tab says about it once it has. */
export interface ConnectRequest {
  config: ConnectionConfig;
  keyringRef: string | null;
  /** Where the tab points once connected: an entry's id, the form's `editingId`, or null. */
  savedId: string | null;
  /** The tab's title; when absent, `name`, or the engine and host. */
  title?: string;
  /** The form's name box, as typed. */
  name: string;
}

interface Options {
  onTitleChange: (title: string) => void;
  onStateChange: (state: unknown) => void;
}

/** A connection's lifecycle: connecting, the tunnel test, disconnecting, and what a closed tab
 *  leaves behind. It never reads the form — what it dials is handed to `connect`. */
export function useConnection({ onTitleChange, onStateChange }: Options) {
  const { t } = useTranslation();
  const [connectionId, setConnectionId] = useState<string | null>(null);
  const [status, setStatus] = useState("");
  const [error, setError] = useState("");
  const [tunnelStatus, setTunnelStatus] = useState<TunnelStatus | null>(null);

  // Closing a tab unmounts this component, and the backend has no other way of hearing about it:
  // without this the pool (and the SSH tunnel behind it) would stay open in `AppState` until the
  // app itself is quit, one leaked connection per tab ever opened.
  //
  // Read through a ref so the effect can depend on nothing and therefore only ever run its cleanup
  // on a real unmount — depending on `connectionId` would disconnect on every change of it, which
  // includes the moment a connection is established.
  const connectionIdRef = useRef<string | null>(null);
  useEffect(() => {
    connectionIdRef.current = connectionId;
  }, [connectionId]);
  /** Whether this tab is gone. The cleanup below can only disconnect what has already arrived, and
   *  a `connect_db` still in flight has not: the id lands in a closure nobody is left to read, and
   *  the pool and the tunnel behind it stay in `DbState` until the app quits. `connect` reads this
   *  when its await comes back and hangs up on itself. */
  const closedRef = useRef(false);
  useEffect(() => {
    /* Lowered on the way *in*, not only raised on the way out. A ref outlives the effect, and in
       StrictMode the first mount is thrown away — setup, cleanup, setup again — so a flag only
       ever raised would be left raised on a tab that is very much alive, and every connection it
       opened would be hung up on the moment it arrived. */
    closedRef.current = false;
    return () => {
      closedRef.current = true;
      const id = connectionIdRef.current;
      // Nothing is left to show an error to, and a connection the backend has already forgotten is
      // not a failure worth reporting anywhere.
      if (id) disconnectDb(id).catch(() => {});
    };
  }, []);

  /* One dial at a time. The state disables the button; the ref is what a second call reads, because
     the two ways in do not both go through a click — `openAndConnect` runs from an effect and from
     a row's double click, and a state set moments earlier is not there yet in either. Without it a
     second connection opens under the same tab and the first id is overwritten in `setConnectionId`,
     leaving a pool nobody can name and only the app quitting can close. */
  const connectingRef = useRef(false);
  const [connecting, setConnecting] = useState(false);

  /** What the form is told about the connection, cleared: the first half of what loading another
   *  entry wipes — `useConnectionForm` clears its own `stale`. */
  const clearFeedback = useCallback(() => {
    setError("");
    setStatus("");
    setTunnelStatus(null);
  }, []);

  const isClosed = useCallback(() => closedRef.current, []);

  async function testTunnel(ssh: ConnectionConfig["ssh"]) {
    setTunnelStatus({ tone: "pending", message: t("connection.testingTunnel") });
    if (!ssh) {
      setTunnelStatus(null);
      return;
    }
    try {
      await testSshTunnel(ssh);
      setTunnelStatus({ tone: "ok", message: t("connection.tunnelOk") });
    } catch (e) {
      setTunnelStatus({ tone: "error", message: t("connection.tunnelFailed", { error: errorMessage(t, e) }) });
    }
  }

  /**
   * Opens the connection and puts the workspace on screen.
   *
   * `savedId` is a parameter and not merely a read of `editingId`: `openAndConnect` applies the
   * saved connection to the form and calls this in the same tick, so `setEditingId` has not taken
   * effect and this closure still sees whatever was there before. The Connect button passes
   * `editingId` itself (`connectFromForm` in `DbTab`), or the commonest path of all (click a
   * connection, press Connect) would leave the tab pointing at nothing and reopening blank next
   * launch.
   *
   * `keyringRef` follows the config it belongs to: the form's when the form is what dials, the
   * entry's when `openAndConnect` passes one, and none for a handoff, which brought its password.
   */
  async function connect({ config: typed, keyringRef, savedId, title, name }: ConnectRequest) {
    if (connectingRef.current) return;
    connectingRef.current = true;
    setConnecting(true);
    setError("");
    setStatus(t("connection.connecting"));
    try {
      // MixEngine's password is read here and nowhere earlier: a launch asks the Keychain about
      // MixLab's vault alone (ADR 0056), and this is the moment someone asked to connect.
      const config = await withResolvedPassword(typed, keyringRef);
      const id = await connectDb(config);
      /* The tab was closed while this was dialling — over an SSH tunnel to a server that is slow to
         answer, that is a wait long enough to close a tab in. This is the only moment anyone knows
         the id: the unmount cleanup ran while it was still `null`, and nothing else will ever be
         told about it. */
      if (closedRef.current) {
        disconnectDb(id).catch(() => {});
        return;
      }
      setConnectionId(id);
      /* Where the tab points is whatever the form is holding: the entry passed in, or the one
         loaded in the sidebar. Written on both branches, so it does not depend on `disconnect`
         having run first — connecting to something else replaces it, and connecting to a config
         nobody saved clears it. An id and a flag — see `tabState.ts`.

         A connection loaded and then edited by hand still points at what it was loaded from. The
         edit is not saved anywhere and cannot be restored; the name in the box and the row marked
         in the sidebar both say that entry, so the tab says it too. */
      onStateChange(savedId === null ? undefined : { savedId, connected: true });
      setStatus(t("connection.connectedStatus", { id: id.slice(0, 8) }));
      /* What a tab with no name of its own says it is pointing at. Each kind keeps its address
         somewhere different: Mongo inside the connection string, SQLite in a path — whose last
         segment is the useful half, since the tab is a word or two wide — and the rest in `host`,
         which for those two is empty. */
      const titleHost =
        config.kind === "mongo"
          ? mongoUriHost(config.uri ?? "")
          : config.kind === "sqlite"
            ? fileName(config.path ?? "")
            : config.host;
      onTitleChange(
        title ??
          (name.trim() ||
            // The engine as it is named everywhere else in the app, not the wire value: a tab
            // called "postgres · db" beside a sidebar row marked PostgreSQL is the app
            // disagreeing with itself.
            t("connection.fallbackTitle", { kind: t(kindLabel(config.kind)), host: titleHost })),
      );
    } catch (e) {
      /* The state is left alone. A server that is off, or a VPN that is not up, is not the user
         having moved away from that connection — the banner says so and the tab still points
         where it pointed. */
      if (closedRef.current) return;
      setStatus("");
      setError(errorMessage(t, e));
    } finally {
      connectingRef.current = false;
      setConnecting(false);
    }
  }

  /**
   * Leave this connection: the workspace goes, the form comes back.
   *
   * The form comes back holding this same connection, and the sidebar still has it marked — the
   * one thing not cleared here is `editingId`. So the way back in is Connect, not finding it in
   * the list again, which is what makes this reversible enough to reach from a tab strip.
   *
   * The backend is told and not listened to, the same bargain the unmount above strikes: a
   * connection it has already forgotten is not a reason to keep a dead workspace on screen, and
   * leaving was the user's decision rather than a request that can be refused.
   */
  async function disconnect(entry: SavedConnection | undefined) {
    if (!connectionId) return;
    await disconnectDb(connectionId).catch(() => {});
    setConnectionId(null);
    setStatus("");
    /* Named after what the form is holding, which is what every other tab in the app is named
       after and what disconnecting has not changed: the saved connection is still loaded, still
       marked in the sidebar, and Connect is still all it takes to go back in. A tab that renamed
       itself "New Connection" over the top of that was the one thing on screen disagreeing with
       the rest of it. The entry's name and not the name box, which may be holding an edit nobody
       has saved — that has not renamed the connection, only proposed a new name for it.

       Nothing saved is loaded when a connection was typed in by hand, and then the form really
       is a new one and says so. */
    onTitleChange(entry?.name ?? t("app.newConnectionTitle"));
    /* And the tab keeps pointing at it, with the flag turned down. Forgetting it here is what used
       to make a restarted app disagree with itself: the tab came back still named after the
       connection — because that name is in the session — but holding an empty form, because
       nothing said which connection the name belonged to. Next launch it comes back the way it
       looks now, at the form with the connection loaded, and Connect is still all it takes. */
    onStateChange(entry === undefined ? undefined : { savedId: entry.id, connected: false });
  }

  return {
    connectionId,
    connecting,
    status,
    setStatus,
    error,
    setError,
    tunnelStatus,
    clearFeedback,
    isClosed,
    testTunnel,
    connect,
    disconnect,
  };
}

export type Connection = ReturnType<typeof useConnection>;
