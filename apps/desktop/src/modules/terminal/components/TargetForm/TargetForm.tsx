import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import Button from "../../../../components/Button";
import Input, { Textarea } from "../../../../components/Input";
import NoticeBanner from "../../../../components/NoticeBanner";
import Select from "../../../../components/Select";
import SegmentedControl from "../../../../components/SegmentedControl";
import { errorMessage } from "../../../../core/errors";
import { savedChange } from "../../../../core/followSaved";
import { DEFAULT_SSH_PORT, PRIVATE_KEY_PLACEHOLDER } from "../../../../core/ssh";
import { stableStringify } from "../../../../core/stableStringify";
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
import type { DraftTarget } from "../../tabState";
import { formatEnvLines, parseEnvLines, parsePathLines } from "../../targetEnv";
import type {
  LocalShell,
  OnRestore,
  SavedTarget,
  SshAuth,
  SshConfig,
  TerminalChoice,
} from "../../types";
import SavedTargetList from "./SavedTargetList";
import styles from "./TargetForm.module.css";

/**
 * A saved target reduced to exactly "what it is" — without `id`.
 *
 * Leaving out `id` is what makes *Save as new* comparable: the new entry has a different id but
 * identical content, and the Update button has to read "nothing changed" right after. It goes
 * through `stableStringify` because the key order of a freshly built form object and of the same
 * object after a round trip through JSON are not the same — see `core/stableStringify.ts`.
 */
function snapshotOf(target: SavedTarget): string {
  const { id: _id, ...body } = target;
  return stableStringify(body);
}

interface Props {
  onOpen: (choice: TerminalChoice) => void;
  onError: (message: string) => void;
  /** The tab that was just tried and failed. The form rebuilds exactly what the user typed — a form
   *  wiped clean after every wrong password is a form nobody can use. */
  initial: TerminalChoice | null;
  /** A target another module drafted, shown unsaved for the person to check — T205, D11. Read
   *  once, when the form mounts. */
  draft?: DraftTarget | null;
  /** The draft has been saved as an entry of its own. */
  onDraftSaved?: () => void;
}

/** The screen a terminal tab shows before there is a session: pick this machine or a server. */
function TargetForm({ onOpen, onError, initial, draft = null, onDraftSaved }: Props) {
  const { t } = useTranslation();
  const targets = useSavedTargets();
  const targetsLoaded = useSavedTargetsLoaded();

  const [kind, setKind] = useState<"local" | "ssh">(draft ? "local" : (initial?.kind ?? "local"));

  /** The saved target the form is holding, or `null` when it was typed by hand. */
  const [targetId, setTargetId] = useState<string | null>(initial?.targetId ?? null);
  const [name, setName] = useState(draft?.name ?? "");
  /** A snapshot of the saved target when it was loaded, so the Update button knows whether
   *  anything has changed. */
  const [savedSnapshot, setSavedSnapshot] = useState<string | null>(null);
  const [runOnConnect, setRunOnConnect] = useState(
    draft?.runOnConnect ?? initial?.runOnConnect ?? "",
  );
  /** The draft this form opened on, while it is still unsaved: it says so above the form. */
  const [drafted, setDrafted] = useState(draft !== null);
  /** The saved entry this form holds changed or went away somewhere else, while it held edits. */
  const [stale, setStale] = useState<"changed" | "removed" | null>(null);

  // This machine
  const [shells, setShells] = useState<LocalShell[]>([]);
  /* The name rather than the path, both in state and as the `Select`'s value: the name is what goes
     to disk, so a saved row can be loaded into the form even before the machine's shell list has
     finished reading. */
  const [shellName, setShellName] = useState(
    draft ? draft.shellName : initial?.kind === "local" ? initial.shell.name : "",
  );
  const [cwd, setCwd] = useState(
    draft ? draft.cwd : initial?.kind === "local" ? (initial.cwd ?? "") : "",
  );
  /* What the shell is given beside its directory, as the lines the person types — T205, D9. */
  const [envText, setEnvText] = useState(
    draft
      ? formatEnvLines(draft.env)
      : initial?.kind === "local"
        ? formatEnvLines(initial.env ?? undefined)
        : "",
  );
  const [pathText, setPathText] = useState(
    draft
      ? draft.pathPrepend.join("\n")
      : initial?.kind === "local"
        ? (initial.pathPrepend ?? []).join("\n")
        : "",
  );
  /** What a tab restored on the next launch does with *Run on connect* — T205, D10. Not part of a
   *  session, so it is read off the saved entry once that is found. */
  const [onRestore, setOnRestore] = useState<OnRestore>(draft?.onRestore ?? "run");
  /* A draft from a blueprint nobody vouches for keeps its `onRestore` until it is saved: saving is
     the person putting their name to it. */
  const restoreLocked = drafted && draft?.lockRestore === true;

  // SSH
  const [host, setHost] = useState(initial?.kind === "ssh" ? initial.config.host : "");
  const [port, setPort] = useState(initial?.kind === "ssh" ? initial.config.port : DEFAULT_SSH_PORT);
  const [username, setUsername] = useState(initial?.kind === "ssh" ? initial.config.username : "");
  const [authType, setAuthType] = useState<"password" | "privatekey">(
    initial?.kind === "ssh" ? initial.config.auth.type : "password",
  );
  const [password, setPassword] = useState(
    initial?.kind === "ssh" && initial.config.auth.type === "password"
      ? initial.config.auth.password
      : "",
  );
  const [keyPath, setKeyPath] = useState(
    initial?.kind === "ssh" && initial.config.auth.type === "privatekey"
      ? initial.config.auth.key_path
      : "",
  );
  const [passphrase, setPassphrase] = useState(
    initial?.kind === "ssh" && initial.config.auth.type === "privatekey"
      ? (initial.config.auth.passphrase ?? "")
      : "",
  );

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
        /* `current ||` keeps what `initial` set: a failed opening followed by a retry has to return
           to the shell just tried, not to the default shell. If the default shell has been removed
           from the machine, `preferred` is `undefined` and the first one Rust suggests takes its
           place. */
        setShellName((current) => current || preferred?.name || (found[0]?.name ?? ""));
        const dir = settings.defaultCwd;
        if (dir) setCwd((current) => current || dir);
      })
      .catch((e) => onError(errorMessage(t, e)));
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
   *
   * Only the name and snapshot are set, deliberately not overwriting the whole entry: what the user
   * just typed has to stay as it is. A form that, after an opening failed on a wrong password, puts
   * the old password back by itself is a form arguing with the user. And because the snapshot is
   * taken from the entry rather than from the form, a just-edited password brings the Update button
   * back to life — exactly as it should.
   *
   * Waits for `targetsLoaded` because the list is empty until the file has been read, and before
   * that every id looks deleted.
   */
  useEffect(() => {
    if (namedInitial.current || targetId === null || !targetsLoaded) return;
    namedInitial.current = true;
    const entry = targets.find((target) => target.id === targetId);
    if (entry === undefined) {
      /* That row was deleted while the session was running. The form keeps everything it has — it
         can still reopen — but stops pointing at an id with nothing behind it, so the button no
         longer says "Update" about an entry that does not exist. */
      setTargetId(null);
      return;
    }
    setName(entry.name);
    setOnRestore(entry.onRestore ?? "run");
    setSavedSnapshot(snapshotOf(entry));
  }, [targetsLoaded, targets]);

  /** `applyTarget` as of the last render, for the effect below: a function made anew each render
   *  cannot be one of its dependencies without making it run on every render. */
  const applyLatest = useRef(applyTarget);
  useLayoutEffect(() => {
    applyLatest.current = applyTarget;
  });

  /* The variables as typed, or the first line that is not `KEY=value`: such a form neither saves
     nor opens, and says which line. Declared above the first `buildTarget` call, which runs
     during render: below it, they would not exist yet. */
  const parsedEnv = parseEnvLines(envText);
  const envError = "error" in parsedEnv ? parsedEnv.error : null;
  const env = "env" in parsedEnv && Object.keys(parsedEnv.env).length > 0 ? parsedEnv.env : undefined;
  const pathPrepend = parsePathLines(pathText);

  /* The saved entry this form holds can change under it: sync brings a newer one, or another tab
     updates or deletes it. An untouched form follows; edits are the person's and stay, and they
     are told. One that went away leaves the form as it is, and Save makes it a new one. */
  const heldEntry = targetId === null ? undefined : targets.find((target) => target.id === targetId);
  const currentSnapshot = heldEntry && snapshotOf(heldEntry);
  const heldSnapshot = snapshotOf(buildTarget(""));
  useEffect(() => {
    if (!namedInitial.current || targetId === null || !targetsLoaded) return;
    const change = savedChange(currentSnapshot, savedSnapshot, heldSnapshot);
    if (change === "reload" && heldEntry) applyLatest.current(heldEntry);
    else if (change === "changed") setStale("changed");
    else if (change === "removed") {
      setTargetId(null);
      setSavedSnapshot(null);
      setStale("removed");
    }
  }, [targetId, targetsLoaded, heldEntry, currentSnapshot, savedSnapshot, heldSnapshot]);

  const chosenShell = shells.find((shell) => shell.name === shellName);

  async function browseDirectory() {
    const picked = await openDialog({ directory: true, multiple: false });
    if (typeof picked === "string") setCwd(picked);
  }

  async function browseKeyFile() {
    const picked = await openDialog({ directory: false, multiple: false });
    if (typeof picked === "string") setKeyPath(picked);
  }

  function buildAuth(): SshAuth {
    return authType === "password"
      ? { type: "password", password }
      : { type: "privatekey", key_path: keyPath, passphrase: passphrase || undefined };
  }

  function buildConfig(): SshConfig {
    return { host: host.trim(), port, username: username.trim(), auth: buildAuth() };
  }

  /** Which target the form describes, under the id passed in. An empty field is written as
   *  `undefined` rather than `""`: absence is the default, so an entry not using that field does
   *  not carry a dead line around. `onRestore` likewise: `run` is what an absent one means. */
  function buildTarget(id: string): SavedTarget {
    const trimmedName = name.trim();
    const opening = runOnConnect.trim() || undefined;
    const restore = onRestore === "run" ? {} : { onRestore };
    if (kind === "local") {
      return {
        id,
        name: trimmedName,
        kind: "local",
        shellName,
        cwd: cwd.trim() || null,
        runOnConnect: opening,
        ...restore,
        ...(env === undefined ? {} : { env }),
        ...(pathPrepend.length === 0 ? {} : { pathPrepend }),
      };
    }
    return {
      id,
      name: trimmedName,
      kind: "ssh",
      config: buildConfig(),
      runOnConnect: opening,
      ...restore,
    };
  }

  /** Enough for an attempt to mean something: an address, a user, and whatever the chosen
   *  authentication method needs. */
  const sshReady =
    host.trim() !== "" &&
    username.trim() !== "" &&
    (authType === "password" ? password !== "" : keyPath.trim() !== "");

  /** What is being opened, built from the form. `null` when the form is not yet enough for an
   *  attempt to mean something. */
  function buildChoice(): TerminalChoice | null {
    const opening = runOnConnect.trim() || null;
    if (kind === "local") {
      return chosenShell && envError === null
        ? {
            kind: "local",
            shell: chosenShell,
            cwd: cwd.trim() || null,
            targetId,
            runOnConnect: opening,
            press: true,
            env: env ?? null,
            pathPrepend: pathPrepend.length === 0 ? null : pathPrepend,
          }
        : null;
    }
    return sshReady
      ? { kind: "ssh", config: buildConfig(), targetId, runOnConnect: opening, press: true }
      : null;
  }

  /** Enough to save: a name, for this machine a real shell whose name can be saved, and variables
   *  that read. */
  const savable =
    name.trim() !== "" &&
    (kind === "ssh" || (chosenShell !== undefined && envError === null));

  /* The Update button is dead when the form holds exactly what was saved. Only asked when there is
     an entry to compare against: for a hand-typed target the button is Save, and Save is never
     pointless. */
  const unchanged = targetId !== null && savedSnapshot === snapshotOf(buildTarget(""));

  function applyTarget(entry: SavedTarget) {
    setStale(null);
    setDrafted(false);
    // The targets column is always there, even while the form is on the other kind — clicking a
    // row without the form switching kind would look like the click did nothing.
    setKind(entry.kind);
    setTargetId(entry.id);
    setName(entry.name);
    setRunOnConnect(entry.runOnConnect ?? "");
    setOnRestore(entry.onRestore ?? "run");
    setSavedSnapshot(snapshotOf(entry));
    // The name has been looked for already, and it is this one. The effect above has no turn left
    // to overwrite it.
    namedInitial.current = true;
    if (entry.kind === "local") {
      setShellName(entry.shellName);
      setCwd(entry.cwd ?? "");
      setEnvText(formatEnvLines(entry.env));
      setPathText((entry.pathPrepend ?? []).join("\n"));
      /* And clear the other branch. Without clearing, switching to the SSH tab afterwards would
         show the address and password of another server — the one loaded before — while the left
         column highlights a local row, and the Update button stands ready to turn that row into a
         server. */
      resetSshFields();
      return;
    }
    // Symmetrically: another local row's starting directory has no business here any more. The
    // shell name stays — it belongs to no row; it is what the "This machine" tab opens by default.
    setCwd("");
    setEnvText("");
    setPathText("");
    setHost(entry.config.host);
    setPort(entry.config.port);
    setUsername(entry.config.username);
    setAuthType(entry.config.auth.type);
    setPassword(entry.config.auth.type === "password" ? entry.config.auth.password : "");
    setKeyPath(entry.config.auth.type === "privatekey" ? entry.config.auth.key_path : "");
    setPassphrase(
      entry.config.auth.type === "privatekey" ? (entry.config.auth.passphrase ?? "") : "",
    );
  }

  /** Double-clicking a row: loads it into the form and opens it right away. The config is taken
   *  straight from the clicked entry rather than from state — `applyTarget` has just called
   *  `setState`, and state only changes on the next render. */
  function openTarget(entry: SavedTarget) {
    applyTarget(entry);
    const opening = entry.runOnConnect ?? null;
    if (entry.kind === "ssh") {
      onOpen({
        kind: "ssh",
        config: entry.config,
        targetId: entry.id,
        runOnConnect: opening,
        press: true,
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
      runOnConnect: opening,
      press: true,
      env: entry.env ?? null,
      pathPrepend: entry.pathPrepend ?? null,
    });
  }

  /** Clears the SSH half of the form. Separate because `applyTarget` needs it too: loading a local
   *  row while leaving the credentials of the server just viewed would leave them sitting behind a
   *  tab only one click away. */
  function resetSshFields() {
    setHost("");
    setPort(DEFAULT_SSH_PORT);
    setUsername("");
    setAuthType("password");
    setPassword("");
    setKeyPath("");
    setPassphrase("");
  }

  /** Clears the form. The selected kind stays: "+" means "a new one of the kind I am looking at",
   *  and both kinds can be saved now. */
  function clearForm() {
    setStale(null);
    setDrafted(false);
    setTargetId(null);
    setName("");
    setSavedSnapshot(null);
    setRunOnConnect("");
    setOnRestore("run");
    setCwd("");
    setEnvText("");
    setPathText("");
    resetSshFields();
  }

  /** A saved draft is an entry like any other from here on. */
  function settleDraft() {
    if (!drafted) return;
    setDrafted(false);
    onDraftSaved?.();
  }

  async function saveTarget() {
    if (!savable) return;
    try {
      const entry = buildTarget(targetId ?? crypto.randomUUID());
      if (targetId) {
        await updateTarget(entry);
      } else {
        await addTarget(entry);
        setTargetId(entry.id);
      }
      setSavedSnapshot(snapshotOf(entry));
      settleDraft();
    } catch (e) {
      onError(errorMessage(t, e));
    }
  }

  /** The same content, a new id, and the form switches to holding the copy — what was saved stays
   *  exactly as it was. */
  async function saveAsNew() {
    if (!savable) return;
    try {
      const entry = buildTarget(crypto.randomUUID());
      await addTarget(entry);
      setTargetId(entry.id);
      setSavedSnapshot(snapshotOf(entry));
      settleDraft();
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

  const choice = buildChoice();

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

      <div className={styles.form}>
        {/* The two kinds of target. Buttons rather than a `Select`: there are only two, and the
            chosen one decides the whole rest of the form — worth seeing both at once. */}
        <SegmentedControl
          block
          mode="tabs"
          aria-label={t("terminal.newTabTitle")}
          value={kind}
          onChange={setKind}
          segments={[
            { value: "local", label: t("terminal.targetLocal") },
            { value: "ssh", label: t("terminal.targetSsh") },
          ]}
        />

        {drafted && <NoticeBanner message={t("terminal.draftNotice")} />}

        {/* Outside both branches: both kinds can be saved, so both have a name. */}
        <div className={styles.row}>
          <label htmlFor="terminal-target-name">{t("terminal.targetName")}</label>
          <Input
            id="terminal-target-name"
            value={name}
            placeholder={
              kind === "local"
                ? t("terminal.targetNamePlaceholderLocal")
                : t("terminal.targetNamePlaceholderSsh")
            }
            onChange={(e) => setName(e.target.value)}
          />
        </div>

        {kind === "local" ? (
          <>
            <div className={styles.row}>
              {/* `Select` does not take an `id`, so its label is `ariaLabel` rather than
                  `htmlFor` */}
              <span>{t("terminal.shell")}</span>
              <Select
                value={shellName}
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
                onChange={setShellName}
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
                  value={cwd}
                  placeholder={t("terminal.startInPlaceholder")}
                  onChange={(e) => setCwd(e.target.value)}
                />
                <Button onClick={() => void browseDirectory()}>{t("terminal.browse")}</Button>
              </div>
            </div>

            <div className={styles.row}>
              <label htmlFor="terminal-env">{t("terminal.envLabel")}</label>
              <Textarea
                id="terminal-env"
                value={envText}
                onChange={(e) => setEnvText(e.target.value)}
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
                value={pathText}
                onChange={(e) => setPathText(e.target.value)}
              />
              <p className={styles.hint}>{t("terminal.pathHint")}</p>
            </div>
          </>
        ) : (
          <>
            <div className={styles.columns}>
              <div className={styles.row}>
                <label htmlFor="terminal-host">{t("terminal.host")}</label>
                <Input id="terminal-host" value={host} onChange={(e) => setHost(e.target.value)} />
              </div>
              <div className={`${styles.row} ${styles.narrow}`}>
                <label htmlFor="terminal-port">{t("terminal.port")}</label>
                <Input
                  id="terminal-port"
                  type="number"
                  value={port}
                  onChange={(e) => setPort(Number(e.target.value))}
                />
              </div>
            </div>

            <div className={styles.row}>
              <label htmlFor="terminal-user">{t("terminal.username")}</label>
              <Input
                id="terminal-user"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
              />
            </div>

            <div className={styles.row}>
              <span>{t("terminal.authMethod")}</span>
              <Select
                value={authType}
                options={[
                  { value: "password", label: t("terminal.authPassword") },
                  { value: "privatekey", label: t("terminal.authPrivateKey") },
                ]}
                onChange={(value) => setAuthType(value)}
                ariaLabel={t("terminal.authMethod")}
              />
            </div>

            {authType === "password" ? (
              <div className={styles.row}>
                <label htmlFor="terminal-password">{t("terminal.password")}</label>
                <Input
                  id="terminal-password"
                  type="password"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                />
              </div>
            ) : (
              <>
                <div className={styles.row}>
                  <label htmlFor="terminal-key">{t("terminal.privateKeyFile")}</label>
                  <div className={styles.withButton}>
                    <Input
                      id="terminal-key"
                      value={keyPath}
                      placeholder={PRIVATE_KEY_PLACEHOLDER}
                      onChange={(e) => setKeyPath(e.target.value)}
                    />
                    <Button onClick={() => void browseKeyFile()}>{t("terminal.browse")}</Button>
                  </div>
                </div>
                <div className={styles.row}>
                  <label htmlFor="terminal-passphrase">{t("terminal.keyPassphrase")}</label>
                  <Input
                    id="terminal-passphrase"
                    type="password"
                    value={passphrase}
                    onChange={(e) => setPassphrase(e.target.value)}
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
            value={runOnConnect}
            placeholder={t("terminal.runOnConnectPlaceholder")}
            onChange={(e) => setRunOnConnect(e.target.value)}
          />
          <p className={styles.hint}>{t("terminal.runOnConnectHint")}</p>
        </div>

        {/* For both kinds, and only meaningful with something to run: a tab restored on the next
            launch runs it, types it and waits, or just opens — T205, D10. */}
        <div className={styles.row}>
          <span>{t("terminal.onRestoreLabel")}</span>
          <SegmentedControl
            aria-label={t("terminal.onRestoreLabel")}
            value={onRestore}
            onChange={setOnRestore}
            segments={(["run", "type", "none"] as const).map((value) => ({
              value,
              label: t(
                value === "run"
                  ? "terminal.onRestoreRun"
                  : value === "type"
                    ? "terminal.onRestoreType"
                    : "terminal.onRestoreNone",
              ),
              disabled: runOnConnect.trim() === "" || restoreLocked,
            }))}
          />
          {restoreLocked && <p className={styles.hint}>{t("terminal.restoreLocked")}</p>}
        </div>

        {stale === "changed" && (
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
        {stale === "removed" && (
          <NoticeBanner message={t("terminal.removedElsewhere")} onDismiss={() => setStale(null)} />
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
            {kind === "local" ? t("terminal.open") : t("terminal.connect")}
          </Button>
        </div>
      </div>
    </div>
  );
}

export default TargetForm;
