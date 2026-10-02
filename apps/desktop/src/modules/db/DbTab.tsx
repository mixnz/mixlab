import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  addConnection,
  removeConnection,
  updateConnection,
  useSavedConnections,
  useSavedConnectionsLoaded,
} from "./savedConnectionsStore";
import type { ConnectionConfig, DbKind, SavedConnection } from "./types";
import { reconnectsOnRestore, withResolvedPassword } from "./savedConnections";
import { parseDbTabState } from "./tabState";
import { takeHandoff } from "./handoff";
import { arrivesConnected } from "./handoffArrival";
import { scrollTopFor } from "./savedListScroll";
import SqlWorkspace from "./sql/SqlWorkspace";
import { SqlProvider } from "./sql/context";
import { SQL_ENGINES, isSqlKind } from "./engines";
import ConnectionForm from "./components/ConnectionForm";
import {
  configFrom,
  formFrom,
  kindLabel,
  withKind,
  type ConnectionForm as FormState,
} from "./connectionForm";
import MongoWorkspace from "./mongo/MongoWorkspace";
import RedisWorkspace from "./redis/RedisWorkspace";
import Button from "../../components/Button";
import EmptyState from "../../components/EmptyState";
import ErrorBanner from "../../components/ErrorBanner";
import NoticeBanner from "../../components/NoticeBanner";
import ContextMenu from "../../components/ContextMenu";
import FilterChip, { FilterChipRow } from "../../components/FilterChip";
import Input from "../../components/Input";
import { LockIcon, PinIcon, PlusIcon } from "../../icons";
import EngineBadge from "./components/EngineBadge";
import { connectionPlace, engineCounts, filterConnections } from "./connectionString";
import { DatabaseIcon } from "./icons";
import { useTranslation } from "../../i18n";
import { errorMessage } from "../../core/errors";
import { savedChange } from "../../core/followSaved";
import { fileName } from "../../core/paths";
import { stableStringify } from "../../core/stableStringify";
import type { ModuleTabProps, TabBadge } from "../../shell/module";
import { dbBadgeMarks } from "./badges";
/* The module's own global stylesheet: the connection form, the saved list and the three
   workspaces. Component-scoped rules live in each component's CSS Module. */
import "./db.css";




// The Mongo form is a single connection string, so the two things the rest of the app used to
// read off separate fields — the host for a tab title, the database to open first — have to be
// picked back out of it. Both are cosmetic: an unparseable string yields nothing and the
// connection itself still reports the real error.
// Split by hand rather than with `new URL`: a comma-separated seed list —
// `mongodb://a:27017,b:27017/?replicaSet=rs0` — is a perfectly good Mongo string but not a valid
// URL, and that shape is exactly the one a replica set is written in.
const MONGO_URI_RE = /^mongodb(?:\+srv)?:\/\/(?:[^@/]*@)?([^/?]*)(?:\/([^?]*))?/i;

/** Host only — the string itself carries the password, which must never reach a tab title. */
function mongoUriHost(uri: string): string {
  return MONGO_URI_RE.exec(uri.trim())?.[1] ?? "";
}

/** The default database, i.e. the path segment in `mongodb://host/thisOne?options`. */
function mongoUriDatabase(uri: string): string {
  const path = MONGO_URI_RE.exec(uri.trim())?.[2] ?? "";
  try {
    return decodeURIComponent(path);
  } catch {
    return path;
  }
}




/**
 * What the tunnel test is currently saying. The tone travels beside the sentence rather than being
 * read back out of it: the three messages are translated, so the text itself can't be matched on.
 */
interface TunnelStatus {
  tone: "pending" | "ok" | "error";
  message: string;
}

function DbTab({ active, onTitleChange, onBadgesChange, restored, onStateChange }: ModuleTabProps) {
  const { t, lang } = useTranslation();
  /* The whole form as one value — see `connectionForm.ts`, where the two directions live. What
     was seventeen `useState` calls, and two lists of seventeen setters that had to be kept in
     step by hand. */
  const [form, setForm] = useState(() => formFrom(null));
  /* The tab reads three of the eighteen for itself: the kind and host name a connection in the
     title, the database is what a workspace opens on, and the tunnel decides who reports a lost
     connection. The rest of the eighteen belongs to the form and stays there. */
  const { kind, uri, database, tunnelType } = form;

  /** One field of the form, changed. What the form component reports back through.
   *
   *  Editing `password` by hand clears `keyringRef`: what is now in the box is no longer what the
   *  reference points at, so Save must go back to writing a plain copy rather than the old
   *  address. */
  function set<K extends keyof FormState>(field: K, value: FormState[K]) {
    setForm((current) => ({
      ...current,
      [field]: value,
      ...(field === "password" ? { keyringRef: null } : {}),
    }));
  }

  const savedConnections = useSavedConnections();
  const [saveAsName, setSaveAsName] = useState("");
  const [editingId, setEditingId] = useState<string | null>(null);
  const [savedSnapshot, setSavedSnapshot] = useState<string | null>(null);
  const [contextMenu, setContextMenu] = useState<{ id: string; x: number; y: number } | null>(null);

  const [connectionId, setConnectionId] = useState<string | null>(null);
  /* What this tab was connected to last launch, taken once. A snapshot and not a live read: the
     moment this tab connects it writes a new value, and reading that back would be the tab
     restoring itself from itself. */
  const [restoredState] = useState(() => parseDbTabState(restored));
  const savedConnectionsLoaded = useSavedConnectionsLoaded();
  /** Whether the restore below has had its one turn — win or lose. Without it, another tab saving
   *  a connection publishes a new list, the effect runs again, and this tab opens a second
   *  connection to the same server. */
  const restoreTried = useRef(false);
  const [status, setStatus] = useState("");
  const [error, setError] = useState("");
  /** The saved connection this form took changed or went away somewhere else, while it held edits. */
  const [stale, setStale] = useState<"changed" | "removed" | null>(null);
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
      if (id) invoke("disconnect_db", { id }).catch(() => {});
    };
  }, []);

  /* One dial at a time. The state disables the button; the ref is what a second call reads, because
     the two ways in do not both go through a click — `openAndConnect` runs from an effect and from
     a row's double click, and a state set moments earlier is not there yet in either. Without it a
     second connection opens under the same tab and the first id is overwritten in `setConnectionId`,
     leaving a pool nobody can name and only the app quitting can close. */
  const connectingRef = useRef(false);
  const [connecting, setConnecting] = useState(false);
  /** How many times the form has been asked to put the caret in the password field — see
   *  `ConnectionForm`'s `focusPassword`. Bumped for a handed-over connection that came without one. */
  const [focusPassword, setFocusPassword] = useState(0);

  /**
   * Whether the tab bar should be showing a lock for this tab.
   *
   * Only once connected: before that the mark belongs to the row in the sidebar, and the form on
   * screen may be for another connection entirely. Read off the store rather than remembered from
   * the click, so clearing the flag in one tab takes the lock off this one too.
   */
  const activeReadOnly = Boolean(
    connectionId && savedConnections.find((c) => c.id === editingId)?.readOnly,
  );

  /**
   * The marks this tab asks the tab bar for: the engine's logo, and the lock when the connection
   * behind it is read-only.
   *
   * The shell is told what to draw rather than what this tab is — it has no `DbKind` and no notion
   * of read-only. Which marks a given state calls for is {@link dbBadgeMarks}; turning them into
   * icons and sentences is here, because this is the side that has a `t`.
   */
  const badges = useMemo<TabBadge[]>(
    () =>
      dbBadgeMarks(connectionId ? kind : undefined, activeReadOnly).map((mark) =>
        mark.type === "kind"
          ? {
              id: "kind",
              // The same logo the sidebar row carries, without the tinted tile around it: a tab has
              // no room for a badge, and the mark alone is what has to be recognised.
              icon: <DatabaseIcon kind={mark.kind} size={14} />,
              label: t(kindLabel(mark.kind)),
              className: `tab-kind kind-${mark.kind}`,
            }
          : {
              id: "readOnly",
              icon: <LockIcon size={12} />,
              label: t("common.readOnly"),
              title: t("common.readOnlyConnection"),
              className: "tab-lock",
              // The bar along the top of the open tab goes amber, which is the one thing on screen
              // that says so no matter which pane of the workspace is showing.
              tabClassName: "tab-readonly",
            },
      ),
    [connectionId, kind, activeReadOnly, t],
  );
  useEffect(() => {
    onBadgesChange(badges);
  }, [badges]);

  function changeKind(next: DbKind) {
    setForm((current) => withKind(current, next));
  }


  /**
   * Wipes everything the form is saying back to the user. Each message is about the connection that
   * was in the form when it appeared — a failed connect, a tunnel test — so loading a different one
   * leaves it describing something that is no longer on screen.
   */
  function clearFeedback() {
    setError("");
    setStatus("");
    setTunnelStatus(null);
    setStale(null);
  }

  /** The saved entry again, over what the form holds: what sync brought, or another tab saved. The
   *  tab keeps what it is doing, so nothing here touches the connection or the restore state. */
  const reloadSaved = useCallback(
    (entry: SavedConnection) => {
      setForm(formFrom(entry.config, entry.keyringRef ?? null));
      setSaveAsName(entry.name);
      setSavedSnapshot(stableStringify({ name: entry.name, config: entry.config }));
      setStale(null);
      onTitleChange(entry.name);
    },
    [onTitleChange],
  );

  function applySavedConnection(entry: SavedConnection) {
    setForm(formFrom(entry.config, entry.keyringRef ?? null));
    setEditingId(entry.id);
    setSaveAsName(entry.name);
    setSavedSnapshot(stableStringify({ name: entry.name, config: entry.config }));
    clearFeedback();
    onTitleChange(entry.name);
    /* Beside the title, and for the same reason: from here on the tab is named after this
       connection, so next launch has to know which one that was. `connect` overwrites it a tick
       later when this is on the way in through `openAndConnect` — until then, and for a form the
       user is only looking at, the flag is honest about there being no connection open. */
    onStateChange({ savedId: entry.id, connected: false });
  }

  function newConnectionForm() {
    setEditingId(null);
    setSaveAsName("");
    setSavedSnapshot(null);
    setForm(formFrom(null));
    clearFeedback();
    onTitleChange(t("app.newConnectionTitle"));
    // Nothing to point at any more, and the title says so too.
    onStateChange(undefined);
  }


  async function saveConnection() {
    const name = saveAsName.trim();
    if (!name) return;
    if (editingId) {
      // Everything the entry carries that the form doesn't — whether it is pinned, the sidebar
      // width, Redis's scan limit — is kept: saving a connection edits its settings, it doesn't
      // reset the rest of what is remembered about it.
      const existing = savedConnections.find((c) => c.id === editingId);
      const config = configFrom(form);
      const entry: SavedConnection = {
        ...existing,
        id: editingId,
        name,
        config,
        keyringRef: form.keyringRef ?? undefined,
      };
      await updateConnection(entry);
      setSavedSnapshot(stableStringify({ name: entry.name, config: entry.config }));
    } else {
      const entry: SavedConnection = {
        id: crypto.randomUUID(),
        name,
        config: configFrom(form),
        keyringRef: form.keyringRef ?? undefined,
      };
      await addConnection(entry);
      setEditingId(entry.id);
      setSavedSnapshot(stableStringify({ name: entry.name, config: entry.config }));
    }
  }

  async function saveConnectionAsNew() {
    const name = saveAsName.trim();
    if (!name) return;
    const entry: SavedConnection = {
      id: crypto.randomUUID(),
      name,
      config: configFrom(form),
      keyringRef: form.keyringRef ?? undefined,
    };
    await addConnection(entry);
    setEditingId(entry.id);
    setSavedSnapshot(stableStringify({ name: entry.name, config: entry.config }));
  }

  async function deleteSavedConnection(id: string) {
    await removeConnection(id);
    if (editingId === id) {
      newConnectionForm();
    }
  }

  async function duplicateSavedConnection(id: string) {
    const source = savedConnections.find((c) => c.id === id);
    if (!source) return;
    const entry: SavedConnection = {
      id: crypto.randomUUID(),
      name: t("connection.copySuffix", { name: source.name }),
      config: source.config,
      // Carried over, unlike `pinned`: a copy of a production connection points at the same
      // server, so the mark that says not to write to it has to come along. Losing it would
      // turn "duplicate" into the one click that makes a read-only connection writable.
      readOnly: source.readOnly,
      // Carried over too: the copy is still the same MixEngine account, so it should still save
      // as a reference rather than as a plaintext copy of the password.
      keyringRef: source.keyringRef,
    };
    await addConnection(entry);
  }

  async function togglePinned(id: string) {
    const entry = savedConnections.find((c) => c.id === id);
    if (!entry) return;
    // Unpinning drops the flag rather than writing `false`: absent is the default, so a connection
    // that was pinned once doesn't carry a dead `"pinned": false` in the file forever.
    await updateConnection({ ...entry, pinned: entry.pinned ? undefined : true });
  }

  async function toggleReadOnly(id: string) {
    const entry = savedConnections.find((c) => c.id === id);
    if (!entry) return;
    // Dropped rather than written as `false`, for the same reason `pinned` is.
    await updateConnection({ ...entry, readOnly: entry.readOnly ? undefined : true });
  }

  /**
   * The list as the sidebar shows it: pinned first, alphabetical within each half.
   *
   * Sorted for display only — what is on disk keeps the order it was written in, so renaming a
   * connection doesn't rewrite the file's shape. `Intl.Collator` rather than comparing strings
   * with `<`: it files accented names beside their plain spelling instead of after every
   * unaccented one, orders `db2` before `db10`, and follows the language the app is set to.
   */
  const orderedConnections = useMemo(() => {
    const collator = new Intl.Collator(lang, { sensitivity: "base", numeric: true });
    return [...savedConnections].sort((a, b) => {
      if (Boolean(a.pinned) !== Boolean(b.pinned)) return a.pinned ? -1 : 1;
      return collator.compare(a.name, b.name);
    });
  }, [savedConnections, lang]);

  /* The saved list narrowed by a name search and by engine chips — presentation over the list
     already in hand, see `connectionString.ts`. */
  const [listQuery, setListQuery] = useState("");
  const [listKinds, setListKinds] = useState<ReadonlySet<DbKind>>(new Set());
  const engines = useMemo(() => engineCounts(orderedConnections), [orderedConnections]);
  const shownConnections = useMemo(
    () => filterConnections(orderedConnections, { query: listQuery, kinds: listKinds }),
    [orderedConnections, listQuery, listKinds],
  );

  function toggleListKind(engine: DbKind) {
    setListKinds((current) => {
      const next = new Set(current);
      if (next.has(engine)) next.delete(engine);
      else next.add(engine);
      return next;
    });
  }

  function clearListFilters() {
    setListQuery("");
    setListKinds(new Set());
  }
  function openContextMenu(e: React.MouseEvent, id: string) {
    e.preventDefault();
    setContextMenu({ id, x: e.clientX, y: e.clientY });
  }

  function closeContextMenu() {
    setContextMenu(null);
  }


  async function testTunnel() {
    setTunnelStatus({ tone: "pending", message: t("connection.testingTunnel") });
    const ssh = configFrom(form).ssh;
    if (!ssh) {
      setTunnelStatus(null);
      return;
    }
    try {
      await invoke("test_ssh_tunnel", { ssh });
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
   * effect and this closure still sees whatever was there before. It is an override rather than
   * the only source — the Connect button passes nothing and has to fall back to `editingId`, or
   * the commonest path of all (click a connection, press Connect) would leave the tab pointing at
   * nothing and reopening blank next launch.
   *
   * `keyringRef` follows the config it belongs to: the form's when the form is what dials, the
   * entry's when `openAndConnect` passes one, and none for a handoff, which brought its password.
   */
  async function connect(
    overrideConfig?: ConnectionConfig,
    title?: string,
    savedId?: string,
    keyringRef: string | null = overrideConfig ? null : form.keyringRef,
  ) {
    if (connectingRef.current) return;
    // Read the form before the flag goes up: anything that threw between the two would leave the
    // flag raised with no `finally` to lower it, and the tab stuck on "Connecting…" for good.
    const typed = overrideConfig ?? configFrom(form);
    connectingRef.current = true;
    setConnecting(true);
    setError("");
    setStatus(t("connection.connecting"));
    try {
      // MixEngine's password is read here and nowhere earlier: a launch asks the Keychain about
      // MixLab's vault alone (ADR 0056), and this is the moment someone asked to connect.
      const config = await withResolvedPassword(typed, keyringRef);
      const id = await invoke<string>("connect_db", { config });
      /* The tab was closed while this was dialling — over an SSH tunnel to a server that is slow to
         answer, that is a wait long enough to close a tab in. This is the only moment anyone knows
         the id: the unmount cleanup ran while it was still `null`, and nothing else will ever be
         told about it. */
      if (closedRef.current) {
        invoke("disconnect_db", { id }).catch(() => {});
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
      const pointsAt = savedId ?? editingId;
      onStateChange(pointsAt === null ? undefined : { savedId: pointsAt, connected: true });
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
          (saveAsName.trim() ||
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

  function openAndConnect(entry: SavedConnection) {
    applySavedConnection(entry);
    connect(entry.config, entry.name, entry.id, entry.keyringRef ?? null);
  }

  /* The tab coming back to what it had open, once, the first time it is looked at — which for a
     tab restored from the last session is the first time it is mounted at all.
     `savedConnectionsLoaded` is the whole reason this waits: the list is empty until the file has
     been read, and acting on it before then would read "the connection was deleted" every launch.
     A connection that really has gone leaves the form as it opens, with no banner — nothing failed.

     A tab that was in its workspace dials again; one the user had disconnected only loads the
     connection into the form and marks it in the sidebar. Both come back looking like what was
     left behind, which is the whole of the promise — the second half of it used to be missing, and
     that tab opened blank under a title naming a connection it no longer pointed at.

     `openAndConnect` and `applySavedConnection` are deliberately not dependencies; they are rebuilt
     every render.

     A tab whose password is MixEngine's comes back as its form even if it was connected: dialling
     it would be a second Keychain question before anybody asked to connect. */
  useEffect(() => {
    if (restoreTried.current || restoredState === null || !("savedId" in restoredState) || !savedConnectionsLoaded) return;
    restoreTried.current = true;
    const entry = savedConnections.find((c) => c.id === restoredState.savedId);
    if (entry === undefined) return;
    if (restoredState.connected && reconnectsOnRestore(entry)) openAndConnect(entry);
    else applySavedConnection(entry);
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
        if (closedRef.current) return;
        setForm(formFrom(handoff.config, handoff.keyring_ref ?? null));
        setSaveAsName(handoff.label);
        onTitleChange(handoff.label);
        if (arrivesConnected(handoff.config)) {
          void connect(handoff.config, handoff.label);
        } else {
          setStatus(t("connection.handoffNeedsPassword"));
          setFocusPassword((count) => count + 1);
        }
      },
      () => {},
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps -- runs once per tab, on the snapshot taken at mount; `connect` and the two callbacks are rebuilt every render.
  }, [restoredState]);

  /* The list, its sticky title and the row marked as the one being edited — read by the effect
     below, which is the only thing that touches them. */
  const savedListRef = useRef<HTMLElement>(null);
  const savedHeaderRef = useRef<HTMLDivElement>(null);
  const activeRowRef = useRef<HTMLLIElement>(null);
  /* Where this tab's list was last scrolled to, kept while a workspace replaces it, and the list
     element the effect below last saw — a different one is the list having been mounted again. */
  const savedListScrollRef = useRef<number | null>(null);
  const mountedListRef = useRef<HTMLElement | null>(null);
  /* What puts the marked row where it is in the list — its group and its name. The effect below
     follows the row when this changes (it was renamed or pinned) and not on any other change to the
     list: pinning, duplicating or deleting another connection, or a sync bringing one in, would
     otherwise pull a list the user had scrolled away back to the marked row. */
  const editedSortKey = useMemo(() => {
    const entry = savedConnections.find((c) => c.id === editingId);
    return entry ? `${entry.pinned ? 1 : 0}|${entry.name}` : null;
  }, [savedConnections, editingId]);

  /* The connection the form is holding, brought into view in the list beside it.

     The app coming back up on a connection saved sixty names down the alphabet renders this list
     from nothing, scrolled to the top, with the marked row somewhere below the fold — the mark is
     there and says nothing to anyone who cannot see it.

     Disconnect mounts the list again too, but that list has been looked at before: it goes back to
     where this tab left it, not to wherever the marked row would put it. The row is already on
     screen if that is where the user had it, and if they had scrolled away, that was their choice.

     A layout effect, so the list is already in the right place the first time it is painted rather
     than jumping there afterwards. `connectionId` is a dependency because leaving a workspace is
     what mounts this list again; `savedConnectionsLoaded` because the list is empty until the file
     has been read, and the row to scroll to does not exist before then. `scrollTopFor` answers
     `null` for a row already in view, so clicking around the list scrolls nothing. */
  useLayoutEffect(() => {
    const list = savedListRef.current;
    if (list === null) {
      mountedListRef.current = null;
      return;
    }
    if (mountedListRef.current === null) {
      mountedListRef.current = list;
      if (savedListScrollRef.current !== null) {
        list.scrollTop = savedListScrollRef.current;
        return;
      }
    }
    const row = activeRowRef.current;
    if (row === null) return;
    const top = row.getBoundingClientRect().top - list.getBoundingClientRect().top + list.scrollTop;
    const target = scrollTopFor(
      { top, height: row.offsetHeight },
      { scrollTop: list.scrollTop, height: list.clientHeight },
      savedHeaderRef.current?.offsetHeight ?? 0,
    );
    if (target !== null) list.scrollTop = target;
  }, [connectionId, editingId, savedConnectionsLoaded, editedSortKey]);

  /* The saved connection this form took can change under it: sync brings a newer one, or another
     tab saves or deletes it. An untouched form follows; edits are the person's and stay, and they
     are told. One that went away leaves the form as it is, and Save makes it a new one, rather than
     an update to an id that no longer exists, which would save nothing. */
  useEffect(() => {
    if (editingId === null || !savedConnectionsLoaded) return;
    const entry = savedConnections.find((c) => c.id === editingId);
    const current = entry && stableStringify({ name: entry.name, config: entry.config });
    const held = stableStringify({ name: saveAsName.trim(), config: configFrom(form) });
    const change = savedChange(current, savedSnapshot, held);
    if (change === "reload" && entry) reloadSaved(entry);
    else if (change === "changed") setStale("changed");
    else if (change === "removed") {
      setEditingId(null);
      setSavedSnapshot(null);
      setStale("removed");
    }
  }, [editingId, savedConnectionsLoaded, savedConnections, savedSnapshot, saveAsName, form, reloadSaved]);

  async function updateSidebarWidth(width: number) {
    if (!editingId) return;
    const entry = savedConnections.find((c) => c.id === editingId);
    if (!entry || entry.sidebarWidth === width) return;
    await updateConnection({ ...entry, sidebarWidth: width });
  }

  async function updateRedisScanLimit(limit: number) {
    if (!editingId) return;
    const entry = savedConnections.find((c) => c.id === editingId);
    if (!entry || entry.redisScanLimit === limit) return;
    await updateConnection({ ...entry, redisScanLimit: limit });
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
  async function disconnect() {
    if (!connectionId) return;
    await invoke("disconnect_db", { id: connectionId }).catch(() => {});
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
    const entry = savedConnections.find((c) => c.id === editingId);
    onTitleChange(entry?.name ?? t("app.newConnectionTitle"));
    /* And the tab keeps pointing at it, with the flag turned down. Forgetting it here is what used
       to make a restarted app disagree with itself: the tab came back still named after the
       connection — because that name is in the session — but holding an empty form, because
       nothing said which connection the name belonged to. Next launch it comes back the way it
       looks now, at the form with the connection loaded, and Connect is still all it takes. */
    onStateChange(entry === undefined ? undefined : { savedId: entry.id, connected: false });
  }


  if (!connectionId) {
    return (
      <div className="login-view">
        <aside
          className="saved-list"
          ref={savedListRef}
          onScroll={(e) => {
            savedListScrollRef.current = e.currentTarget.scrollTop;
          }}
        >
          {/* Title, search and chips stay put while the rows scroll under them — one sticky block,
              whose height is what `scrollTopFor` keeps a scrolled-to row clear of. */}
          <div className="saved-list-header" ref={savedHeaderRef}>
            <div className="saved-list-title">
              <h3>{t("connection.connections")}</h3>
              <span className="saved-list-count">{savedConnections.length}</span>
              {/* Creating a connection is an action on the list, not one of its rows, so it sits in
                  the header where it stays reachable however far the names scroll. */}
              <Button
                size="small"
                variant="soft"
                className="saved-list-new"
                onClick={newConnectionForm}
                title={t("connection.newConnection")}
                aria-label={t("connection.newConnection")}
              >
                <PlusIcon size={15} />
              </Button>
            </div>
            <Input
              allowClear
              value={listQuery}
              onChange={(e) => setListQuery(e.target.value)}
              placeholder={t("connection.searchConnections")}
              aria-label={t("connection.searchConnections")}
            />
            {engines.length > 1 && (
              <FilterChipRow>
                {engines.map(([engine, count]) => (
                  <FilterChip
                    key={engine}
                    compact
                    pressed={listKinds.has(engine)}
                    count={count}
                    title={t(kindLabel(engine))}
                    leading={<DatabaseIcon kind={engine} size="0.95em" className={`choice-icon kind-${engine}`} />}
                    onClick={() => toggleListKind(engine)}
                  >
                    {t(kindLabel(engine))}
                  </FilterChip>
                ))}
              </FilterChipRow>
            )}
          </div>
          {savedConnections.length > 0 && shownConnections.length === 0 ? (
            <EmptyState
              title={t("connection.noConnectionsMatch")}
              action={
                <Button size="small" onClick={clearListFilters}>
                  {t("connection.clearFilters")}
                </Button>
              }
            />
          ) : (
            [
              { key: "pinned", label: t("connection.pinnedGroup"), rows: shownConnections.filter((c) => c.pinned) },
              { key: "all", label: t("connection.connections"), rows: shownConnections.filter((c) => !c.pinned) },
            ]
              // The Pinned group only when something is pinned; the other always, so an empty list
              // still reads as a list.
              .filter((group) => group.key === "all" || group.rows.length > 0)
              .map((group) => (
                <section key={group.key} className="saved-group">
                  <h4 className="saved-group-label">
                    {group.label}
                    <span>{group.rows.length}</span>
                  </h4>
                  <ul>
                    {group.rows.map((c) => (
                      <li key={c.id} ref={c.id === editingId ? activeRowRef : null}>
                        <button
                          type="button"
                          className={`saved-item${c.id === editingId ? " saved-item-active" : ""}${
                            c.readOnly ? " saved-item-readonly" : ""
                          }`}
                          onClick={() => applySavedConnection(c)}
                          onDoubleClick={() => openAndConnect(c)}
                          onContextMenu={(e) => openContextMenu(e, c.id)}
                          title={t("connection.savedItemTooltip")}
                        >
                          {/* The engine's own logo, in its own tile: the row is recognised by a
                              shape the user already knows from everywhere else. The engine's name
                              is written in the line under the connection's. */}
                          <EngineBadge kind={c.config.kind} size={30} />
                          <span className="saved-item-text">
                            <strong>{c.name}</strong>
                            <span className="saved-item-sub">
                              {t(kindLabel(c.config.kind))}
                              {connectionPlace(c.config) !== "" && ` · ${connectionPlace(c.config)}`}
                            </span>
                          </span>
                          {/* Read-only is about what the row will let you do, so it says the word
                              rather than only drawing a lock — a shape alone would be one more badge
                              to learn. */}
                          {c.readOnly && (
                            <span className="saved-item-readonly-badge">
                              <LockIcon size={12} />
                              {t("common.readOnly")}
                            </span>
                          )}
                          {/* Says why this one sits above the alphabet. The button's own `title`
                              describes the row, so the mark carries its word in a `<span>` for
                              screen readers rather than in a second tooltip that would replace it. */}
                          {c.pinned && (
                            <span className="saved-item-pin">
                              <PinIcon size={14} />
                              <span className="visually-hidden">{t("connection.pinnedTooltip")}</span>
                            </span>
                          )}
                        </button>
                      </li>
                    ))}
                  </ul>
                </section>
              ))
          )}
        </aside>
        <section className="login-form">
          <ConnectionForm
            form={form}
            onChange={set}
            onKindChange={changeKind}
            editingId={editingId}
            name={saveAsName}
            onNameChange={setSaveAsName}
            /* A name is needed, and an edit that changes nothing is not a save. Worked out here
               rather than in the form, because it is a question about the saved list. */
            saveDisabled={
              !saveAsName.trim() ||
              (editingId !== null &&
                savedSnapshot === stableStringify({ name: saveAsName.trim(), config: configFrom(form) }))
            }
            status={status}
            connecting={connecting}
            tunnelStatus={tunnelStatus}
            focusPassword={focusPassword}
            notices={
              <>
                {error && <ErrorBanner message={error} onDismiss={() => setError("")} />}
                {stale === "changed" && (
                  <div className="saved-elsewhere">
                    <NoticeBanner message={t("connection.changedElsewhere")} />
                    <Button
                      size="small"
                      onClick={() => {
                        const entry = savedConnections.find((c) => c.id === editingId);
                        if (entry) reloadSaved(entry);
                      }}
                    >
                      {t("connection.loadNewVersion")}
                    </Button>
                  </div>
                )}
                {stale === "removed" && (
                  <NoticeBanner message={t("connection.removedElsewhere")} onDismiss={() => setStale(null)} />
                )}
              </>
            }
            onSave={saveConnection}
            onSaveAsNew={saveConnectionAsNew}
            onConnect={() => connect()}
            onTestTunnel={testTunnel}
          />
        </section>
        {contextMenu && (
          <ContextMenu x={contextMenu.x} y={contextMenu.y} onClose={closeContextMenu}>
            <button
              type="button"
              onClick={() => {
                togglePinned(contextMenu.id);
                closeContextMenu();
              }}
            >
              {savedConnections.find((c) => c.id === contextMenu.id)?.pinned
                ? t("connection.unpin")
                : t("connection.pin")}
            </button>
            {/* Offered whatever the kind: each workspace closes off everything of its own that
                would write, so the flag means the same thing on a Mongo or Redis connection as it
                does on a MySQL one. */}
            <button
              type="button"
              onClick={() => {
                toggleReadOnly(contextMenu.id);
                closeContextMenu();
              }}
            >
              {savedConnections.find((c) => c.id === contextMenu.id)?.readOnly
                ? t("connection.allowWrites")
                : t("connection.markReadOnly")}
            </button>
            <button
              type="button"
              onClick={() => {
                duplicateSavedConnection(contextMenu.id);
                closeContextMenu();
              }}
            >
              {t("common.duplicate")}
            </button>
            <button
              type="button"
              className="context-menu-delete"
              onClick={() => {
                deleteSavedConnection(contextMenu.id);
                closeContextMenu();
              }}
            >
              {t("common.delete")}
            </button>
          </ContextMenu>
        )}
      </div>
    );
  }

  if (isSqlKind(kind)) {
    const activeSavedConnection = savedConnections.find((c) => c.id === editingId);
    // The one place the engine behind a SQL workspace is chosen. Everything below reaches for it
    // through the context rather than importing an engine of its own — see `src/sql/context.tsx`.
    const engine = SQL_ENGINES[kind];
    return (
      <SqlProvider api={engine.api} dialect={engine.dialect}>
        <SqlWorkspace
          active={active}
          connectionId={connectionId}
          initialDatabase={database}
          status={status}
          error={error}
          tunnelled={tunnelType === "ssh"}
          onDisconnect={disconnect}
          sidebarWidth={activeSavedConnection?.sidebarWidth}
          onSidebarWidthChange={updateSidebarWidth}
          readOnly={(activeSavedConnection?.readOnly ?? false) || !engine.dialect.writable}
          schemaReadOnly={
            (activeSavedConnection?.readOnly ?? false) || !engine.dialect.ddlWritable
          }
          dataReadOnly={(activeSavedConnection?.readOnly ?? false) || !engine.dialect.rowsWritable}
          dumpRestoreReadOnly={
            (activeSavedConnection?.readOnly ?? false) || !engine.dialect.dumpRestoreWritable
          }
          dmlEvenIfReadOnly={
            !(activeSavedConnection?.readOnly ?? false) && engine.dialect.rowsWritable
          }
          profileId={activeSavedConnection?.id ?? ""}
        />
      </SqlProvider>
    );
  }

  if (kind === "mongo") {
    const activeSavedConnection = savedConnections.find((c) => c.id === editingId);
    return (
      <MongoWorkspace
        active={active}
        connectionId={connectionId}
        initialDatabase={mongoUriDatabase(uri)}
        status={status}
        error={error}
        tunnelled={tunnelType === "ssh"}
        onDisconnect={disconnect}
        sidebarWidth={activeSavedConnection?.sidebarWidth}
        onSidebarWidthChange={updateSidebarWidth}
        readOnly={activeSavedConnection?.readOnly ?? false}
      />
    );
  }

  if (kind === "redis") {
    const activeSavedConnection = savedConnections.find((c) => c.id === editingId);
    return (
      <RedisWorkspace
        active={active}
        connectionId={connectionId}
        initialDatabase={database}
        status={status}
        error={error}
        tunnelled={tunnelType === "ssh"}
        onDisconnect={disconnect}
        sidebarWidth={activeSavedConnection?.sidebarWidth}
        onSidebarWidthChange={updateSidebarWidth}
        scanLimit={activeSavedConnection?.redisScanLimit}
        onScanLimitChange={updateRedisScanLimit}
        readOnly={activeSavedConnection?.readOnly ?? false}
      />
    );
  }

  /* A kind with no workspace to open.
   *
   * Redis used to sit here as the unguarded `else`, which made it the workspace for every kind the
   * three branches above did not claim. That was invisible while they covered every kind, and
   * stopped being invisible the moment one did not: a SQL Server connection opened Redis's key
   * browser, which then asked the backend for Redis things and was told "This is not a Redis
   * connection". The kind reaching here is either one this build knows but cannot browse yet, or
   * one saved by a newer version — see
   * docs/specs/2026-09-05-unknown-db-kind-crash-and-error-logging-design.md, which
   * fixed the icon and the label on that path but not this one.
   *
   * Saying so is the whole of it. Rendering some other engine's workspace is worse than rendering
   * nothing, because it fails somewhere the user cannot connect back to what they picked.
   *
   * `engines.test.ts` mirrors this chain and asserts every kind is claimed by exactly one branch;
   * the two must agree, so a branch added or reordered here is a change there too. */
  return (
    <div className="workspace-unavailable">
      <DatabaseIcon kind={kind} size="2.5rem" className={`kind-${kind}`} />
      <p>{t("connection.workspaceUnavailable", { kind: t(kindLabel(kind)) })}</p>
      <button type="button" onClick={disconnect}>
        {t("common.disconnect")}
      </button>
    </div>
  );
}

export default DbTab;
