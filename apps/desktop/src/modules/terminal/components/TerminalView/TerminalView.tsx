import { useEffect, useRef, useState } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { Terminal } from "@xterm/xterm";
import { openUrl } from "@tauri-apps/plugin-opener";
import ContextMenu from "../../../../components/ContextMenu";
import { copyText } from "../../../../core/clipboard";
import { errorMessage } from "../../../../core/errors";
import { MODIFIER_LABEL, hasPrimaryModifier } from "../../../../core/platform";
import { isClaimed, pressOf, useShortcut } from "../../../../core/shortcuts";
import { useTranslation } from "../../../../i18n";
import {
  closeSession,
  openSession,
  resizeSession,
  writeSession,
  type SessionExit,
} from "../../api";
import { readText } from "../../clipboard";
import { useTerminalSettings, zoomTerminal } from "../../settingsStore";
import { shellKeeps } from "../../keys";
import { openableUrl } from "../../links";
import { openingKeystrokes } from "../../session";
import type { TerminalTarget } from "../../types";
import SearchBar from "../SearchBar";
import { readDocumentToken, searchDecorations, terminalTheme } from "../../xtermTheme";
import styles from "./TerminalView.module.css";

/** Dragging the window produces dozens of events a second; the far end only needs the final
 *  size. */
const RESIZE_DEBOUNCE = 100;

interface Props {
  target: TerminalTarget;
  /** What the saved host's *Run on connect* field holds, or `null`. Typed on the user's behalf
   *  exactly once per session — including a session born from the Reconnect button, since that is
   *  also an entry into that machine. */
  runOnConnect: string | null;
  /** A background tab stays mounted and keeps receiving bytes — this only decides focus and when
   *  to measure again. */
  active: boolean;
  /** The session has finished opening. For SSH this is when connecting, authenticating and
   *  requesting a pty are all done — a few seconds after the button press, so the tab has
   *  something to say while waiting. */
  onOpened: () => void;
  onExit: (exit: SessionExit) => void;
  /** The session could not be opened: a wrong password, a changed fingerprint, an unreachable
   *  server. Unlike `onError`, it says that *there is no session at all*, so the tab returns the
   *  screen to the form. */
  onFailed: () => void;
  /** Drops the ended session for good and goes back to the target picker. Unlike `onExit`, this is
   *  the user speaking, not the shell. */
  onDismiss: () => void;
  onError: (message: string) => void;
}

function TerminalView({
  target,
  runOnConnect,
  active,
  onOpened,
  onExit,
  onFailed,
  onDismiss,
  onError,
}: Props) {
  const { t } = useTranslation();
  const settings = useTerminalSettings();
  const hostRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const searchRef = useRef<SearchAddon | null>(null);
  const sessionRef = useRef<string | null>(null);
  /* In state rather than in a ref: it is the `enabled` condition of `terminal.copy`, and a ref
     changing value re-registers nobody's shortcut. */
  const [hasSelection, setHasSelection] = useState(false);
  /* The shell has died. In state rather than only in the effect's `ended` variable below, for the
     same reason: it decides which shortcuts are registered. */
  const [ended, setEnded] = useState(false);
  /* Whether the find bar is open. In state rather than in a ref: it decides what gets drawn. */
  const [searching, setSearching] = useState(false);
  /** Every time `Ctrl+F` is pressed, even when the bar is already open — see
   *  `SearchBar.focusSignal`. */
  const [findSignal, setFindSignal] = useState(0);
  /** Where the right-click menu is open, in window coordinates. `null` means it is closed. */
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);

  /* The settings used when building the terminal go through a ref: changing a setting resets
     `term.options` in place — see the effect at the end — rather than rebuilding the whole screen
     and reopening the whole session. */
  const settingsRef = useRef(settings);
  settingsRef.current = settings;

  // Callbacks go through a ref: the session-opening effect may only rerun when `target` changes,
  // not on every parent re-render.
  const onOpenedRef = useRef(onOpened);
  onOpenedRef.current = onOpened;
  const onExitRef = useRef(onExit);
  onExitRef.current = onExit;
  const onFailedRef = useRef(onFailed);
  onFailedRef.current = onFailed;
  const onDismissRef = useRef(onDismiss);
  onDismissRef.current = onDismiss;
  const onErrorRef = useRef(onError);
  onErrorRef.current = onError;
  const tRef = useRef(t);
  tRef.current = t;
  const runOnConnectRef = useRef(runOnConnect);
  runOnConnectRef.current = runOnConnect;

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const term = new Terminal({
      /* Required, and not for fun: `registerDecoration` — which the search addon calls to highlight
         matches in yellow — is part of the still-proposed API group, and xterm *throws* when this
         flag is off. Without it `findNext` blows up on the first letter the user types into the
         find field, and the whole find bar looks as if it found nothing. */
      allowProposedApi: true,
      fontFamily: settingsRef.current.fontFamily,
      fontSize: settingsRef.current.fontSize,
      cursorStyle: settingsRef.current.cursorStyle,
      cursorBlink: settingsRef.current.cursorBlink,
      scrollback: settingsRef.current.scrollback,
      theme: terminalTheme(readDocumentToken),
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    const search = new SearchAddon();
    term.loadAddon(search);
    /* Addresses on screen open with `Ctrl/Cmd+Click`, not with a plain click: a plain click in a
       terminal places the cursor and starts a selection, and a long log line full of paths would
       become a minefield if every touch launched the browser. `hasPrimaryModifier` rather than
       `ctrlKey || metaKey` — on a Mac `Ctrl+Click` is the context-menu click, and it has to keep
       that meaning. What gets opened is `links.ts`'s answer. */
    const links = new WebLinksAddon(
      (event, uri) => {
        if (!hasPrimaryModifier(event)) return;
        const url = openableUrl(uri);
        if (url === null) return;
        void openUrl(url).catch((e) => onErrorRef.current(errorMessage(tRef.current, e)));
      },
      {
        /* xterm underlines everything its regex picks up, including what will not open. This hint
           is the only place that says both things: which key to hold, and whether what is under
           the cursor can be opened. */
        hover: (_event, text) => {
          host.title =
            openableUrl(text) === null
              ? tRef.current("terminal.linkBlocked")
              : tRef.current("terminal.followLink", { modifier: MODIFIER_LABEL });
        },
        leave: () => {
          host.title = "";
        },
      },
    );
    term.loadAddon(links);
    /* The only way to get a key out of xterm. It reads `keydown` on its own hidden textarea and
       `stopPropagation`s every `Ctrl`+letter, so the app's listener on `window` never hears
       `Ctrl+W`. Returning `false` makes xterm bail out *before* `preventDefault`, and the event
       bubbles up intact — to the dispatcher, or to the webview itself when it is a paste. */
    term.attachCustomKeyEventHandler((e) => {
      const press = pressOf(e);
      return shellKeeps(press, isClaimed(press));
    });
    term.open(host);
    fit.fit();
    termRef.current = term;
    fitRef.current = fit;
    searchRef.current = search;

    /* The id is generated here rather than handed out by the tab: `StrictMode` runs effects twice
       in dev, and the first round's cleanup has to close exactly the first round's session. */
    const id = crypto.randomUUID();
    sessionRef.current = id;
    let ended = false;
    /* Whether the startup command has been typed on the user's behalf yet. Waits for the first byte
       rather than sending as soon as `openSession` returns: with SSH, at that point the pty has
       just been granted and the shell on the other side has not printed its prompt yet, and a line
       typed before the shell reads stdin is a line that may be lost — or worse, land in the middle
       of the login banner. The first byte is the shell's first word, and after it, it is
       listening. */
    let greeted = false;
    /* Whether this effect has been cleaned up — the tab closed, or `target` changed and the next
       round has begun. Everything coming back from `openSession` after that is about a session
       nobody is watching any more: writing into a `dispose`d `Terminal` throws, and calling
       `onExit`/`onFailed` now would talk about the old round on the tab the new round just built.
       */
    let disposed = false;

    const typed = term.onData((data) => {
      void writeSession(id, data).catch(() => {});
    });
    const selected = term.onSelectionChange(() => setHasSelection(term.hasSelection()));

    void openSession(id, target, { cols: term.cols, rows: term.rows }, (message) => {
      if (disposed) return;
      if (message instanceof ArrayBuffer) {
        term.write(new Uint8Array(message));
        if (!greeted) {
          greeted = true;
          const keys = openingKeystrokes(runOnConnectRef.current);
          // A failure stays silent, just like any other typing: the session is still open, and the
          // user can keep typing.
          if (keys) void writeSession(id, keys).catch(() => {});
        }
        return;
      }
      ended = true;
      setEnded(true);
      onExitRef.current(message);
    })
      .then(() => {
        /* The tab closed mid-handshake. `terminal_open` only puts the session into the map *after*
           opening, so the cleanup's `closeSession` runs while the map is still empty and closes
           nothing; the session goes into the map right after and from then on nobody knows it
           exists. Closing here is the only place still in time. */
        if (disposed) {
          void closeSession(id).catch(() => {});
          return;
        }
        onOpenedRef.current();
      })
      .catch((e) => {
        /* There is no session to close: `terminal_open` failed before putting anything into the
           map, so the cleanup below must not call `terminal_close` for an id that never existed. */
        ended = true;
        if (disposed) return;
        onErrorRef.current(errorMessage(tRef.current, e));
        onFailedRef.current();
      });

    return () => {
      disposed = true;
      typed.dispose();
      selected.dispose();
      // Only on unmount, not on losing `active`: a background tab still has to keep scrolling.
      if (!ended) void closeSession(id).catch(() => {});
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
      searchRef.current = null;
      sessionRef.current = null;
    };
  }, [target]);

  /* The theme and the accent are attributes on the root element; when either changes, the colours
     xterm was handed are stale, so they are read again. The canvas cannot follow a `var()` itself. */
  useEffect(() => {
    const observer = new MutationObserver(() => {
      const term = termRef.current;
      if (term) term.options.theme = terminalTheme(readDocumentToken);
    });
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme", "data-accent"] });
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    let timer: number | undefined;
    const observer = new ResizeObserver(() => {
      window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        /* A hidden frame has size 0, and `fit()` then computes garbage cols/rows and sends them to
           the server. */
        if (host.clientWidth === 0 || host.clientHeight === 0) return;
        fitRef.current?.fit();
        const term = termRef.current;
        const id = sessionRef.current;
        if (term && id) void resizeSession(id, term.cols, term.rows).catch(() => {});
      }, RESIZE_DEBOUNCE);
    });
    observer.observe(host);

    return () => {
      observer.disconnect();
      window.clearTimeout(timer);
    };
  }, []);

  /* Only registered while there is a selection, and that is the whole way `Ctrl+C` knows which
     command it is: with nothing selected nobody takes the chord, `shellKeeps` hands the key back to
     the shell, and it is the interrupt as always. On macOS `Cmd+C` carries this chord and `Ctrl+C`
     carries nothing — there is nothing to arbitrate. */
  useShortcut("terminal.copy", copySelection, active && hasSelection);

  /* The same `Ctrl/Cmd+C`, and the two are never alive at once: with a selection the key copies,
     and once the session has ended with nothing selected it closes the frozen screen. So the first
     press copies and clears the selection, the second closes — the same rhythm as a live session,
     where the second press is the interrupt. */
  useShortcut("terminal.dismiss", () => onDismissRef.current(), active && ended && !hasSelection);

  /* Zooming goes through the shared store, so every terminal tab changes at once — and because
     `enabled` is `active`, only the tab being viewed takes the key; on a database tab nobody takes
     `Ctrl+=` and it falls through to the webview as before. */
  useShortcut("terminal.zoomIn", () => zoomTerminal(1), active);
  useShortcut("terminal.zoomOut", () => zoomTerminal(-1), active);

  /* Only while the tab is being viewed. On another tab nobody takes `Ctrl+F` and it falls through
     to the webview as before, just as `terminal.zoomIn` does. */
  useShortcut(
    "terminal.find",
    () => {
      setSearching(true);
      setFindSignal((n) => n + 1);
    },
    active,
  );

  function find(query: string, back: boolean): boolean {
    const search = searchRef.current;
    if (!search) return false;
    /* Four colours rather than two: `matchOverviewRuler` and `activeMatchColorOverviewRuler` are
       required in `ISearchDecorationOptions`. They draw the marker strip on the right of the screen
       — which shows where the matches sit across the whole scrolled-back part — so they use the
       same pair of colours as the matches themselves. */
    const options = { decorations: searchDecorations(readDocumentToken) };
    return back ? search.findPrevious(query, options) : search.findNext(query, options);
  }

  function closeSearch() {
    setSearching(false);
    searchRef.current?.clearDecorations();
    // The keyboard goes back to the shell; closing the bar while the cursor stays in a field that
    // has vanished would lose whatever is typed.
    termRef.current?.focus();
  }

  function copySelection() {
    const term = termRef.current;
    if (!term) return;
    const text = term.getSelection();
    if (!text) return;
    /* Clear the selection right away, even if copying failed: leaving it would make the next
       `Ctrl+C` copy again, with no way left to send an interrupt to the shell. */
    term.clearSelection();
    void copyText(text).catch((e) => onErrorRef.current(errorMessage(tRef.current, e)));
  }

  /* The only path that has to read the clipboard itself. `Ctrl+V` does not come through here —
     `shellKeeps` lets the key through to the webview and xterm listens to its own `paste` event —
     but a menu item has no key to let through, so it has to ask the operating system. Through Rust
     rather than `navigator.clipboard`; the reason is in `terminal/clipboard.ts`. */
  async function pasteFromClipboard() {
    const term = termRef.current;
    if (!term) return;
    try {
      const text = await readText();
      // `term.paste` rather than `writeSession` directly: xterm is what knows whether the session
      // has bracketed paste mode on, and a multi-line block pasted into `bash` without the wrapping
      // is a block of commands that runs by itself.
      if (text !== "") term.paste(text);
    } catch (e) {
      onErrorRef.current(errorMessage(tRef.current, e));
    }
  }

  function closeMenu() {
    setMenu(null);
    termRef.current?.focus();
  }

  function handleContextMenu(e: React.MouseEvent) {
    /* The find field is a real text input; the webview's menu on it is the right thing, just as
       `nativeContextMenu.ts` decided for every text input in the app. */
    if (e.target instanceof HTMLInputElement) return;
    /* Required, and this is why: `core/nativeContextMenu.ts` deliberately spares text inputs, and
       xterm's hidden textarea *is* a text input. Without blocking it here what shows up is the
       webview's menu, which includes Reload — one misclick and every open connection drops. */
    e.preventDefault();
    if (settingsRef.current.rightClickPastes) {
      void pasteFromClipboard();
      return;
    }
    setMenu({ x: e.clientX, y: e.clientY });
  }

  /* A font change is a cell size change, so the column and row counts change with it: measure again
     and tell the other end. Without that step `stty size` in the shell says one thing while the
     screen draws another, and everything drawn to the full line width is off.

     The cursor and scrollback change no cell size, but they come along with this effect because
     they come in the same `settings` object: splitting them would be three effects with the same
     dependency. */
  useEffect(() => {
    const term = termRef.current;
    const host = hostRef.current;
    if (!term || !host) return;
    term.options.fontFamily = settings.fontFamily;
    term.options.fontSize = settings.fontSize;
    term.options.cursorStyle = settings.cursorStyle;
    term.options.cursorBlink = settings.cursorBlink;
    term.options.scrollback = settings.scrollback;
    // Leave a hidden frame alone: `fit()` then computes garbage cols/rows. The tab measures again
    // when it comes back — see the `[active]` effect below.
    if (host.clientWidth === 0) return;
    fitRef.current?.fit();
    const id = sessionRef.current;
    if (id) void resizeSession(id, term.cols, term.rows).catch(() => {});
  }, [settings]);

  // The tab is back: the window may have been resized while this frame was hidden, and
  // `ResizeObserver` does not fire for a frame that is `display: none`.
  useEffect(() => {
    if (!active) return;
    const host = hostRef.current;
    const term = termRef.current;
    if (!host || !term || host.clientWidth === 0) return;
    fitRef.current?.fit();
    term.focus();
    const id = sessionRef.current;
    if (id) void resizeSession(id, term.cols, term.rows).catch(() => {});
  }, [active]);

  return (
    <div className={styles.frame} onContextMenu={handleContextMenu}>
      {searching && <SearchBar onFind={find} onClose={closeSearch} focusSignal={findSignal} />}
      <div
        ref={hostRef}
        className={styles.host}
        role="application"
        aria-label={t("terminal.screen")}
      />
      {menu && (
        <ContextMenu x={menu.x} y={menu.y} onClose={closeMenu}>
          <button
            type="button"
            disabled={!hasSelection}
            onClick={() => {
              closeMenu();
              copySelection();
            }}
          >
            {t("terminal.menuCopy")}
          </button>
          <button
            type="button"
            disabled={ended}
            onClick={() => {
              closeMenu();
              void pasteFromClipboard();
            }}
          >
            {t("terminal.menuPaste")}
          </button>
          <button
            type="button"
            onClick={() => {
              closeMenu();
              termRef.current?.selectAll();
            }}
          >
            {t("terminal.menuSelectAll")}
          </button>
          <button
            type="button"
            onClick={() => {
              closeMenu();
              termRef.current?.clear();
            }}
          >
            {t("terminal.menuClear")}
          </button>
        </ContextMenu>
      )}
    </div>
  );
}

export default TerminalView;
