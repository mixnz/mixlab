import { useEffect, useEffectEvent, useReducer, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import Button from "../../../../components/Button";
import Input, { Textarea } from "../../../../components/Input";
import NoticeBanner from "../../../../components/NoticeBanner";
import Select from "../../../../components/Select";
import RadioCard from "../../../../components/RadioCard";
import SegmentedControl from "../../../../components/SegmentedControl";
import { errorMessage } from "../../../../core/errors";
import { savedChange } from "../../../../core/followSaved";
import { PRIVATE_KEY_PLACEHOLDER } from "../../../../core/ssh";
import { useTranslation } from "../../../../i18n";
import { localShells } from "../../api";
import { ShellIcon } from "../../icons";
import {
  addTarget,
  removeTarget,
  updateTarget,
  useSavedTargets,
  useSavedTargetsLoaded,
} from "../../savedTargetsStore";
import { loadTerminalSettings } from "../../settingsStore";
import { shellLabel } from "../../shells";
import { openingFor } from "../../session";
import type { LocalShell, SavedTarget, TerminalChoice } from "../../types";
import SavedTargetList from "./SavedTargetList";
import {
  buildChoice,
  buildTarget,
  draftEnv,
  heldId,
  initialForm,
  isSavable,
  snapshotOf,
  targetForm,
} from "./targetFormState";
import styles from "./TargetForm.module.css";

/** What a restored tab does with *Run on connect* — T205, D10 — as the form offers it. */
const ON_RESTORE_CHOICES = [
  { value: "run", label: "terminal.onRestoreRun", description: "terminal.onRestoreRunHint" },
  { value: "type", label: "terminal.onRestoreType", description: "terminal.onRestoreTypeHint" },
  { value: "none", label: "terminal.onRestoreNone", description: "terminal.onRestoreNoneHint" },
] as const;

interface Props {
  onOpen: (choice: TerminalChoice) => void;
  onError: (message: string) => void;
  /** The tab that was just tried and failed. The form rebuilds exactly what the user typed — a form
   *  wiped clean after every wrong password is a form nobody can use. */
  initial: TerminalChoice | null;
}

/** The screen a terminal tab shows before there is a session: pick this machine or a server. */
function TargetForm({ onOpen, onError, initial }: Props) {
  const { t } = useTranslation();
  const targets = useSavedTargets();
  const targetsLoaded = useSavedTargetsLoaded();

  /* The fields, and the saved entry they hold — see `targetFormState.ts`. One value, so loading a
     row, clearing the form or a change from elsewhere moves them all at once. */
  const [{ draft, held }, dispatch] = useReducer(targetForm, initial, initialForm);
  const { local, ssh } = draft;
  /** The saved target the form is holding, or `null` when it was typed by hand. */
  const targetId = heldId(held);
  const savedSnapshot = held.kind === "held" ? held.snapshot : null;

  const [shells, setShells] = useState<LocalShell[]>([]);

  // `onError` is the parent's and may be a fresh closure each render; the read below runs once.
  const reportError = useEffectEvent((e: unknown) => onError(errorMessage(t, e)));

  useEffect(() => {
    /* Two reads in parallel rather than in sequence, and `loadTerminalSettings` rather than
       `currentTerminalSettings`: the store loads its file asynchronously, so asking it right now
       usually gets the default value rather than the shell the user chose. */
    Promise.all([localShells(), loadTerminalSettings()])
      .then(([found, settings]) => {
        setShells(found);
        const preferred = settings.defaultShell
          ? found.find((shell) => shell.name === settings.defaultShell)
          : undefined;
        /* Into empty fields only: a failed opening followed by a retry has to return to the shell
           just tried, not to the default shell. If the default shell has been removed from the
           machine, `preferred` is `undefined` and the first one Rust suggests takes its place. */
        dispatch({
          type: "defaults",
          shellName: preferred?.name || (found[0]?.name ?? ""),
          cwd: settings.defaultCwd || undefined,
        });
      })
      .catch((e) => reportError(e));
    // Runs only once: a machine's shell list does not change midway.
  }, []);

  /** Whether a name has been looked for for `initial` yet — win or lose, both count, so it only
   *  gets one turn. */
  const namedInitial = useRef(false);

  /**
   * The name and snapshot of the target this tab has just left.
   *
   * `TerminalChoice` carries everything needed to rebuild the form *except the name*: the name
   * belongs to the saved entry, not to the session. Without this step, after leaving a session the
   * form comes back with the right address and password but an empty Name field and a dead Update
   * button — even if the row in the column lights up, nobody reads from that that it is being held.
   * What `named` leaves alone, and why, is in `targetFormState.ts`.
   *
   * Waits for `targetsLoaded` because the list is empty until the file has been read, and before
   * that every id looks deleted.
   *
   * `targetId` is read, not listed: this waits on the list, and an id the user picks or saves later
   * is not the one the tab came back with.
   */
  const currentTargetId = useEffectEvent(() => targetId);
  useEffect(() => {
    const id = currentTargetId();
    if (namedInitial.current || id === null || !targetsLoaded) return;
    namedInitial.current = true;
    dispatch({ type: "named", entry: targets.find((target) => target.id === id) });
  }, [targetsLoaded, targets]);

  const { envError } = draftEnv(draft);

  /* The saved entry this form holds can change under it: sync brings a newer one, or another tab
     updates or deletes it. An untouched form follows; edits are the person's and stay, and they
     are told. One that went away leaves the form as it is, and Save makes it a new one. */
  const heldEntry = targetId === null ? undefined : targets.find((target) => target.id === targetId);
  const currentSnapshot = heldEntry && snapshotOf(heldEntry);
  const heldSnapshot = snapshotOf(buildTarget(draft, ""));
  useEffect(() => {
    if (!namedInitial.current || targetId === null || !targetsLoaded) return;
    const change = savedChange(currentSnapshot, savedSnapshot, heldSnapshot);
    // `namedInitial` is already spent here, so a plain `apply` is all `applyTarget` would do.
    if (change === "reload" && heldEntry) dispatch({ type: "apply", entry: heldEntry });
    else if (change === "changed") dispatch({ type: "changedElsewhere" });
    else if (change === "removed") dispatch({ type: "removedElsewhere" });
  }, [targetId, targetsLoaded, heldEntry, currentSnapshot, savedSnapshot, heldSnapshot]);

  async function browseDirectory() {
    const picked = await openDialog({ directory: true, multiple: false });
    if (typeof picked === "string") dispatch({ type: "editLocal", patch: { cwd: picked } });
  }

  async function browseKeyFile() {
    const picked = await openDialog({ directory: false, multiple: false });
    if (typeof picked === "string") dispatch({ type: "editSsh", patch: { keyPath: picked } });
  }

  const savable = isSavable(draft, shells);

  /* The Update button is dead when the form holds exactly what was saved. Only asked when there is
     an entry to compare against: for a hand-typed target the button is Save, and Save is never
     pointless. */
  const unchanged = targetId !== null && savedSnapshot === heldSnapshot;

  function applyTarget(entry: SavedTarget) {
    dispatch({ type: "apply", entry });
    // The name has been looked for already, and it is this one. The effect above has no turn left
    // to overwrite it.
    namedInitial.current = true;
  }

  /** Double-clicking a row: loads it into the form and opens it right away. The config is taken
   *  straight from the clicked entry rather than from state — `applyTarget` has just dispatched,
   *  and state only changes on the next render. */
  function openTarget(entry: SavedTarget) {
    applyTarget(entry);
    const opening = entry.runOnConnect ?? null;
    if (entry.kind === "ssh") {
      onOpen({
        kind: "ssh",
        config: entry.config,
        targetId: entry.id,
        ...openingFor(opening, entry.onRestore),
      });
      return;
    }
    /* The shell has been removed from the machine — a deleted WSL distro, an uninstalled Git Bash.
       The form has loaded and the shell field is empty, so the user sees right away they have to
       pick another; there is nothing broken to report. */
    const shell = shells.find((s) => s.name === entry.shellName);
    if (shell === undefined) return;
    onOpen({
      kind: "local",
      shell,
      cwd: entry.cwd,
      targetId: entry.id,
      ...openingFor(opening, entry.onRestore),
      env: entry.env ?? null,
      pathPrepend: entry.pathPrepend ?? null,
    });
  }

  /** Clears the form. The selected kind stays: "+" means "a new one of the kind I am looking at",
   *  and both kinds can be saved now. */
  function clearForm() {
    dispatch({ type: "clear" });
  }

  async function saveTarget() {
    if (!savable) return;
    try {
      const entry = buildTarget(draft, targetId ?? crypto.randomUUID());
      if (targetId) await updateTarget(entry);
      else await addTarget(entry);
      dispatch({ type: "saved", entry });
    } catch (e) {
      onError(errorMessage(t, e));
    }
  }

  /** The same content, a new id, and the form switches to holding the copy — what was saved stays
   *  exactly as it was. */
  async function saveAsNew() {
    if (!savable) return;
    try {
      const entry = buildTarget(draft, crypto.randomUUID());
      await addTarget(entry);
      dispatch({ type: "saved", entry });
    } catch (e) {
      onError(errorMessage(t, e));
    }
  }

  async function deleteTarget(id: string) {
    try {
      await removeTarget(id);
      if (targetId === id) clearForm();
    } catch (e) {
      onError(errorMessage(t, e));
    }
  }

  const choice = buildChoice(draft, shells, targetId);

  return (
    <div className={styles.layout}>
      {/* Always shown, not only on the SSH tab: this is the list of places the user goes often, and
          a list that only shows after clicking the right tab saves nobody any clicks. */}
      <SavedTargetList
        targets={targets}
        selectedId={targetId}
        onSelect={applyTarget}
        onOpen={openTarget}
        onDelete={(id) => void deleteTarget(id)}
        onNew={clearForm}
      />

      {/* The form scrolls on its own, beside the list: a pane shorter than the form still reaches
          its buttons. */}
      <div className={styles.formScroll}>
        <div className={styles.form}>
          {/* The two kinds of target. Buttons rather than a `Select`: there are only two, and the
              chosen one decides the whole rest of the form — worth seeing both at once. */}
          <SegmentedControl
            block
            mode="tabs"
            aria-label={t("terminal.newTabTitle")}
            value={draft.kind}
            onChange={(kind) => dispatch({ type: "edit", patch: { kind } })}
            segments={[
              { value: "local", label: t("terminal.targetLocal") },
              { value: "ssh", label: t("terminal.targetSsh") },
            ]}
          />

          {/* Outside both branches: both kinds can be saved, so both have a name. */}
          <div className={styles.row}>
            <label htmlFor="terminal-target-name">{t("terminal.targetName")}</label>
            <Input
              id="terminal-target-name"
              value={draft.name}
              placeholder={
                draft.kind === "local"
                  ? t("terminal.targetNamePlaceholderLocal")
                  : t("terminal.targetNamePlaceholderSsh")
              }
              onChange={(e) => dispatch({ type: "edit", patch: { name: e.target.value } })}
            />
          </div>

          {draft.kind === "local" ? (
            <>
              <div className={styles.row}>
                {/* `Select` does not take an `id`, so its label is `ariaLabel` rather than
                    `htmlFor` */}
                <span>{t("terminal.shell")}</span>
                <Select
                  value={local.shellName}
                  options={shells.map((shell) => ({
                    value: shell.name,
                    label: (
                      <span className={styles.shellOption}>
                        <ShellIcon name={shell.name} />
                        {shellLabel(shell.name)}
                      </span>
                    ),
                    // The label is a node, so the search box cannot read it; this is the text it
                    // reads.
                    searchText: shellLabel(shell.name),
                  }))}
                  onChange={(shellName) => dispatch({ type: "editLocal", patch: { shellName } })}
                  ariaLabel={t("terminal.shell")}
                  /* A machine where no shell was detected, and a saved row pointing at a shell
                     removed from the machine, both show up as an empty field — but they are not the
                     same thing, and "no shell found" is flatly wrong when the list below has five. */
                  placeholder={
                    shells.length === 0 ? t("terminal.noShells") : t("terminal.pickShell")
                  }
                />
              </div>

              <div className={styles.row}>
                <label htmlFor="terminal-cwd">{t("terminal.startIn")}</label>
                <div className={styles.withButton}>
                  <Input
                    id="terminal-cwd"
                    value={local.cwd}
                    placeholder={t("terminal.startInPlaceholder")}
                    onChange={(e) => dispatch({ type: "editLocal", patch: { cwd: e.target.value } })}
                  />
                  <Button onClick={() => void browseDirectory()}>{t("terminal.browse")}</Button>
                </div>
              </div>

              <Button
                variant="link"
                size="small"
                className={styles.advancedToggle}
                aria-expanded={local.advanced}
                onClick={() => dispatch({ type: "editLocal", patch: { advanced: !local.advanced } })}
              >
                {local.advanced ? t("terminal.advancedHide") : t("terminal.advancedShow")}
              </Button>

              {local.advanced && (
                <>
                  <div className={styles.row}>
                    <label htmlFor="terminal-env">{t("terminal.envLabel")}</label>
                    <Textarea
                      id="terminal-env"
                      value={local.envText}
                      onChange={(e) => dispatch({ type: "editLocal", patch: { envText: e.target.value } })}
                    />
                    {envError === null ? (
                      <p className={styles.hint}>{t("terminal.envHint")}</p>
                    ) : (
                      <p className={styles.error} role="alert">
                        {t("terminal.envLineInvalid", { line: envError })}
                      </p>
                    )}
                  </div>

                  <div className={styles.row}>
                    <label htmlFor="terminal-path">{t("terminal.pathLabel")}</label>
                    <Textarea
                      id="terminal-path"
                      value={local.pathText}
                      onChange={(e) => dispatch({ type: "editLocal", patch: { pathText: e.target.value } })}
                    />
                    <p className={styles.hint}>{t("terminal.pathHint")}</p>
                  </div>
                </>
              )}
            </>
          ) : (
            <>
              <div className={styles.columns}>
                <div className={styles.row}>
                  <label htmlFor="terminal-host">{t("terminal.host")}</label>
                  <Input id="terminal-host" value={ssh.host} onChange={(e) => dispatch({ type: "editSsh", patch: { host: e.target.value } })} />
                </div>
                <div className={`${styles.row} ${styles.narrow}`}>
                  <label htmlFor="terminal-port">{t("terminal.port")}</label>
                  <Input
                    id="terminal-port"
                    type="number"
                    value={ssh.port}
                    onChange={(e) => dispatch({ type: "editSsh", patch: { port: Number(e.target.value) } })}
                  />
                </div>
              </div>

              <div className={styles.row}>
                <label htmlFor="terminal-user">{t("terminal.username")}</label>
                <Input
                  id="terminal-user"
                  value={ssh.username}
                  onChange={(e) => dispatch({ type: "editSsh", patch: { username: e.target.value } })}
                />
              </div>

              <div className={styles.row}>
                <span>{t("terminal.authMethod")}</span>
                <Select
                  value={ssh.authType}
                  options={[
                    { value: "password", label: t("terminal.authPassword") },
                    { value: "privatekey", label: t("terminal.authPrivateKey") },
                  ]}
                  onChange={(authType) => dispatch({ type: "editSsh", patch: { authType } })}
                  ariaLabel={t("terminal.authMethod")}
                />
              </div>

              {ssh.authType === "password" ? (
                <div className={styles.row}>
                  <label htmlFor="terminal-password">{t("terminal.password")}</label>
                  <Input
                    id="terminal-password"
                    type="password"
                    value={ssh.password}
                    onChange={(e) => dispatch({ type: "editSsh", patch: { password: e.target.value } })}
                  />
                </div>
              ) : (
                <>
                  <div className={styles.row}>
                    <label htmlFor="terminal-key">{t("terminal.privateKeyFile")}</label>
                    <div className={styles.withButton}>
                      <Input
                        id="terminal-key"
                        value={ssh.keyPath}
                        placeholder={PRIVATE_KEY_PLACEHOLDER}
                        onChange={(e) => dispatch({ type: "editSsh", patch: { keyPath: e.target.value } })}
                      />
                      <Button onClick={() => void browseKeyFile()}>{t("terminal.browse")}</Button>
                    </div>
                  </div>
                  <div className={styles.row}>
                    <label htmlFor="terminal-passphrase">{t("terminal.keyPassphrase")}</label>
                    <Input
                      id="terminal-passphrase"
                      type="password"
                      value={ssh.passphrase}
                      onChange={(e) => dispatch({ type: "editSsh", patch: { passphrase: e.target.value } })}
                    />
                  </div>
                </>
              )}
            </>
          )}

          {/* Outside both branches, just like the Name field: a shell on this machine also has a few
              lines to retype on every opening, and `cd` through the Start in field only covers the
              first of them. */}
          <div className={styles.row}>
            <label htmlFor="terminal-run-on-connect">{t("terminal.runOnConnect")}</label>
            <Textarea
              id="terminal-run-on-connect"
              value={draft.runOnConnect}
              placeholder={t("terminal.runOnConnectPlaceholder")}
              onChange={(e) => dispatch({ type: "edit", patch: { runOnConnect: e.target.value } })}
            />
            <p className={styles.hint}>{t("terminal.runOnConnectHint")}</p>
          </div>

          {/* For both kinds, and only meaningful with something to run: a tab restored on the next
              launch runs it, types it and waits, or just opens — T205, D10. */}
          <div className={styles.row}>
            <span id="terminal-on-restore">{t("terminal.onRestoreLabel")}</span>
            {/* Cards rather than a segmented control: each choice needs its sentence, and three
                labels of different lengths in one strip read as uneven buttons. */}
            <div className={styles.choices} role="radiogroup" aria-labelledby="terminal-on-restore">
              {ON_RESTORE_CHOICES.map(({ value, label, description }) => (
                <RadioCard
                  key={value}
                  name="terminal-on-restore"
                  checked={draft.onRestore === value}
                  disabled={draft.runOnConnect.trim() === ""}
                  onChange={() => dispatch({ type: "edit", patch: { onRestore: value } })}
                  label={t(label)}
                  description={t(description)}
                />
              ))}
            </div>
          </div>

          {held.kind === "held" && held.changedElsewhere && (
            <div className={styles.savedElsewhere}>
              <NoticeBanner message={t("terminal.changedElsewhere")} />
              <Button
                size="small"
                onClick={() => {
                  const entry = targets.find((target) => target.id === targetId);
                  if (entry) applyTarget(entry);
                }}
              >
                {t("terminal.loadNewVersion")}
              </Button>
            </div>
          )}
          {held.kind === "removed" && (
            <NoticeBanner message={t("terminal.removedElsewhere")} onDismiss={() => dispatch({ type: "dismissRemoved" })} />
          )}

          <div className={styles.actions}>
            <Button disabled={!savable || unchanged} onClick={() => void saveTarget()}>
              {targetId ? t("terminal.updateTarget") : t("terminal.saveTarget")}
            </Button>
            {targetId && (
              <Button disabled={!savable} onClick={() => void saveAsNew()}>
                {t("terminal.saveAsNew")}
              </Button>
            )}
            <Button
              variant="primary"
              disabled={choice === null}
              onClick={() => choice && onOpen(choice)}
            >
              {draft.kind === "local" ? t("terminal.open") : t("terminal.connect")}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}

export default TargetForm;
