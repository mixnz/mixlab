import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import Button from "../../components/Button";
import ErrorBanner from "../../components/ErrorBanner";
import { TerminalIcon } from "../../icons";
import type { ModuleTabProps } from "../../shell/module";
import { useTranslation } from "../../i18n";
import { localShells, type SessionExit } from "./api";
import TargetForm from "./components/TargetForm";
import TerminalView from "./components/TerminalView";
import { useSavedTargets, useSavedTargetsLoaded } from "./savedTargetsStore";
import { useTerminalSettings } from "./settingsStore";
import { parseTerminalTabState, tabStateFor } from "./tabState";
import { terminalBadgeMarks, terminalTarget, terminalTitle } from "./session";
import type { TerminalChoice } from "./types";
import "./terminal.css";

/** Terminal: one tab, one session. The form comes first; the session takes its place when the user
 *  presses Open. */
function TerminalTab({ active, onTitleChange, onBadgesChange, restored, onStateChange }: ModuleTabProps) {
  const { t, lang } = useTranslation();
  const [choice, setChoice] = useState<TerminalChoice | null>(null);
  /* The tab that was just tried. Unlike `choice`, it is not cleared when the session fails — the
     form needs it to rebuild exactly what the user typed. */
  const [lastTried, setLastTried] = useState<TerminalChoice | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [exit, setExit] = useState<SessionExit | null>(null);
  /** A session requested but not finished opening. For SSH that is a few seconds of connecting and
   *  authenticating. */
  const [opening, setOpening] = useState(false);
  /* Pressing "Reconnect" bumps this number: `TerminalView` remounts, generates a new id, opens a
     new session. The old content goes with the old instance — rightly, since it is the screen of a
     shell that no longer exists. */
  const [generation, setGeneration] = useState(0);
  /* Where this tab was the last time the app was opened, captured exactly once. Captured rather
     than read live: `start` writes the new value as soon as the session opens, and reading that
     back would make the tab restore itself from itself. */
  const [restoredState] = useState(() => parseTerminalTabState(restored));
  /* Calling `useSavedTargets` here is what starts the read `useSavedTargetsLoaded` is waiting for
     — until now only `TargetForm` called it, and the form is not present while the tab is
     restoring. */
  const savedTargets = useSavedTargets();
  const savedTargetsLoaded = useSavedTargetsLoaded();
  const settings = useTerminalSettings();
  /** Whether the restore has had its turn yet — win or lose, both count. Without this, another tab
   *  saving one more target is a new snapshot, the effect runs again, and this tab opens a second
   *  session. */
  const restoreTried = useRef(false);

  /* The name of the saved target this session came from, when the setting asks for it. With the
     setting off it is `null`, and `terminalTitle` falls back to the old way of naming — so flipping
     the switch renames every open tab at once, just like every other setting in this file. A
     deleted target also gives `null`: the tab keeps the name it has rather than going blank,
     because `find` finds nothing. */
  const savedName =
    settings.titleShowsTargetName && choice?.targetId
      ? (savedTargets.find((target) => target.id === choice.targetId)?.name ?? null)
      : null;

  useEffect(() => {
    onTitleChange(choice ? terminalTitle(choice, savedName) : t("terminal.newTabTitle"));
  // `lang` beside `t`: `t` is one function for the life of the app now, so it is `lang` that says
  // this holds words and has to be built again when they change. See `i18n/index.tsx`.
  }, [choice, savedName, onTitleChange, t, lang]);

  /* `useMemo` rather than building it right in the effect, and the effect does not depend on
     `onBadgesChange`: the shell compares badge lists by reference (see `shell/tabs.ts`), so a new
     array is a `setTabs` — and `App` hands out a new closure on every render. Together those two
     make a self-feeding loop until React cuts it with "Maximum update depth exceeded", taking the
     whole tree with it. */
  const badges = useMemo(
    () =>
      terminalBadgeMarks(choice, exit !== null).map((mark) => {
        if (mark.type === "ended") {
          return {
            id: "ended",
            icon: <TerminalIcon />,
            label: t("terminal.badgeEnded"),
            tabClassName: "terminal-tab-ended",
          };
        }
        return mark.type === "local"
          ? { id: "local", icon: <TerminalIcon />, label: t("terminal.badgeLocal") }
          : { id: "ssh", icon: <TerminalIcon />, label: t("terminal.badgeSsh") };
      }),
    [choice, exit, t],
  );

  useEffect(() => {
    onBadgesChange(badges);
  }, [badges]);

  const showError = useCallback((message: string) => setError(message), []);

  const opened = useCallback(() => setOpening(false), []);

  /* The session could not be opened. `choice` is cleared so the form comes back — with `lastTried`
     intact, so the user fixes the password and presses again rather than retyping from scratch.
     The banner set by `onError` stays on top of it. */
  const failed = useCallback(() => {
    setOpening(false);
    setChoice(null);
  }, []);

  function start(next: TerminalChoice) {
    setLastTried(next);
    setExit(null);
    setOpening(true);
    setChoice(next);
    /* Called from an event handler rather than from render, so a new object each time is right:
       the shell compares by reference and writes only once per session opening. */
    onStateChange(tabStateFor(next));
  }

  /* Drops the dead session and goes back to the target picker. `lastTried` stays, so the form
     rebuilds exactly what was just opened — reopening the old one is one click, and so is switching
     to another.

     This is the only place that forgets the context: pressing this button says "I am leaving
     here". A dead session not yet dismissed is kept — the "session ended" screen with its
     Reconnect button is still that target's screen — and `failed` keeps it too, because a failed
     SSH is not leaving. */
  function dismiss() {
    setExit(null);
    setChoice(null);
    onStateChange(undefined);
  }

  /* The tab returns to exactly where it was, once, the first time it is looked at — for a tab
     restored from the previous session that is also the first time it is mounted.

     The two branches wait for different things. `ssh` waits for the target list to finish reading:
     before that the list is empty and every id looks deleted. `local` waits for `localShells()` —
     shells are detected afresh on every run, so an uninstalled WSL distro or a deleted shell is
     simply not in the list; it only also waits for the target list when it really points at a row.
     Not found goes back to `TargetForm`, with no banner: nothing is broken.

     The startup command is looked up on the live entry rather than read from `localStorage` —
     editing it once makes every tab pointing at it follow. For a deleted entry the `local` branch
     still opens its shell, just with no command left to type on its behalf. */
  useEffect(() => {
    if (restoreTried.current || restoredState === null) return;

    if (restoredState.kind === "ssh") {
      if (!savedTargetsLoaded) return;
      restoreTried.current = true;
      const entry = savedTargets.find((target) => target.id === restoredState.targetId);
      // A row changed from a server to this machine between two app launches: the id is still
      // there but it can no longer open any SSH session, so the tab goes back to the form just as
      // when the entry has been deleted outright.
      if (entry === undefined || entry.kind !== "ssh") return;
      // `config` here is already complete — `savedTargets.ts` merges the secrets from the keyring
      // in before handing it out.
      start({
        kind: "ssh",
        config: entry.config,
        targetId: entry.id,
        runOnConnect: entry.runOnConnect ?? null,
      });
      return;
    }

    /* Only a local tab once saved as a new row has to wait for the list: the other kind has
       nothing to look up, and making it wait would keep it from reopening when the file read
       fails. */
    if (restoredState.targetId !== undefined && !savedTargetsLoaded) return;
    restoreTried.current = true;
    const saved = restoredState.targetId
      ? savedTargets.find((target) => target.id === restoredState.targetId)
      : undefined;
    /* No cancellation flag, on purpose: `restoreTried` already guarantees `localShells()` runs
       exactly once, so the only thing a cleanup could cancel is that very attempt — StrictMode
       unmounts and remounts right after mounting, the cleanup fires before detection finishes,
       and the tab never returns to its shell. Leaving it out costs nothing: if the tab closes
       midway, `start` writes into an unmounted component, React does nothing, and the shell's
       `restateTab` ignores an id no longer in the list. */
    localShells()
      .then((shells) => {
        const shell = shells.find((s) => s.name === restoredState.shellName);
        if (shell === undefined) return;
        start({
          kind: "local",
          shell,
          cwd: restoredState.cwd,
          targetId: saved?.id ?? null,
          runOnConnect: saved?.kind === "local" ? (saved.runOnConnect ?? null) : null,
        });
      })
      // If shell detection fails the tab opens on the form, just as before this feature existed.
      .catch(() => {});
  }, [restoredState, savedTargetsLoaded, savedTargets]);

  function reconnect() {
    setExit(null);
    setOpening(true);
    setGeneration((n) => n + 1);
  }

  /* `useMemo` rather than calling it directly in JSX: `target` is a dependency of the
     session-opening effect in `TerminalView`, so a new object on every parent render is a new
     session on every parent render. */
  const target = useMemo(() => (choice ? terminalTarget(choice) : null), [choice]);

  return (
    // Compact only around a session: the form that picks a target is a form, drawn at the same
    // size as the database module's connection form.
    <div className="terminal-tab" data-density={target ? "compact" : undefined}>
      {error && <ErrorBanner message={error} onDismiss={() => setError(null)} />}
      {target ? (
        <>
          <TerminalView
            key={generation}
            target={target}
            runOnConnect={choice?.runOnConnect ?? null}
            active={active}
            onOpened={opened}
            onExit={setExit}
            onFailed={failed}
            onDismiss={dismiss}
            onError={showError}
          />
          {opening && <div className="terminal-connecting">{t("terminal.connecting")}</div>}
          {exit && (
            <div className="terminal-ended">
              <span>
                {exit.code === null
                  ? t("terminal.sessionEnded")
                  : t("terminal.sessionEndedCode", { code: exit.code })}
              </span>
              <div className="terminal-ended-actions">
                <Button onClick={reconnect}>{t("terminal.reconnect")}</Button>
                <Button onClick={dismiss}>{t("terminal.closeSession")}</Button>
              </div>
            </div>
          )}
        </>
      ) : (
        <TargetForm onOpen={start} onError={showError} initial={lastTried} />
      )}
    </div>
  );
}

export default TerminalTab;
