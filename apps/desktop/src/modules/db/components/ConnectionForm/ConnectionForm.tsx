import { useEffect, useRef, useState, type ReactNode } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import Select from "../../../../components/Select";
import ConfirmDialog from "../../../../components/ConfirmDialog";
import Button from "../../../../components/Button";
import Card from "../../../../components/Card";
import Input from "../../../../components/Input";
import Popover from "../../../../components/Popover";
import SegmentedControl from "../../../../components/SegmentedControl";
import StatusPill from "../../../../components/StatusPill";
import Switch from "../../../../components/Switch";
import { CopyIcon, EyeIcon, EyeOffIcon, MonitorIcon, ServerIcon } from "../../../../icons";
import { copyText } from "../../../../core/clipboard";
import { PRIVATE_KEY_PLACEHOLDER } from "../../../../core/ssh";
import { useTranslation, type TranslationKey } from "../../../../i18n";
import { errorMessage } from "../../../../core/errors";
import { createSqliteFile } from "../../sqlite/api";
import { kindLabel, type ConnectionForm as FormState } from "../../connectionForm";
import { connectionPlace, connectionString } from "../../connectionString";
import type { DbKind } from "../../types";
import EngineBadge from "../EngineBadge";

/**
 * The connection form: what is filled in before Connect, and what a saved connection is edited in.
 *
 * Out of `DbTab` because it was 250 lines of the tab's 1100, and because none of it is about being
 * a tab: it reads one value and reports one field at a time changed. Everything that is only about
 * how the form looks — the mask over a connection string, the example key path, the warning about
 * Redis's protected mode — came out with it, since nothing else was ever asking.
 */

/** Every engine the picker offers, in the order it lists them. */
const ENGINES: DbKind[] = ["mysql", "postgres", "sqlite", "mongo", "redis", "clickhouse", "mssql"];

/** The one line under each engine's name in the picker. */
const ENGINE_DESCRIPTION: Record<DbKind, TranslationKey> = {
  mysql: "connection.engineAboutMysql",
  postgres: "connection.engineAboutPostgres",
  sqlite: "connection.engineAboutSqlite",
  mongo: "connection.engineAboutMongo",
  redis: "connection.engineAboutRedis",
  clickhouse: "connection.engineAboutClickhouse",
  mssql: "connection.engineAboutMssql",
};

function engineDescription(kind: DbKind): TranslationKey {
  return (ENGINE_DESCRIPTION as Partial<Record<string, TranslationKey>>)[kind] ?? "connection.kindUnknown";
}

/**
 * Whether dialling `host` means talking to this machine — the one case a server refusing everything
 * but loopback is happy with. The whole `127.0.0.0/8` block counts, not just `127.0.0.1`, and IPv6
 * writes its loopback either bare or in the brackets a host field may still carry.
 */
function isLoopback(host: string): boolean {
  const name = host.trim().toLowerCase().replace(/^\[|\]$/g, "");
  return (
    name === "localhost" ||
    name === "::1" ||
    /^127(?:\.\d{1,3}){3}$/.test(name)
  );
}

/** Fixed width, not the value's own: the length of a password is itself worth not showing. */
const MASK = "****";

/** The `user:password@` prefix, i.e. everything the string reveals about credentials. */
const MONGO_URI_CREDENTIALS_RE = /^(mongodb(?:\+srv)?:\/\/)([^@/?]+)@/i;

/**
 * What the field shows while hidden. Only the credentials are covered, so the host and options
 * stay readable — those are what you check a connection against at a glance. A string with no
 * credentials in it isn't therefore safe to show: it may be one this app never parsed as Mongo at
 * all, so nothing in it is known to be harmless and the whole value is covered instead.
 */
function maskMongoUri(uri: string): string {
  if (!uri) return "";
  const credentials = MONGO_URI_CREDENTIALS_RE.exec(uri);
  if (!credentials) return MASK;
  const [full, scheme, userinfo] = credentials;
  // `user:password`, `user` alone, and the empty-password `user:` all mask one part per segment.
  const masked = userinfo
    .split(":")
    .map(() => MASK)
    .join(":");
  return `${scheme}${masked}@${uri.slice(full.length)}`;
}

interface Props {
  form: FormState;
  /** One field changed. The only way the inputs below write anything. */
  onChange: <K extends keyof FormState>(field: K, value: FormState[K]) => void;
  /** The kind picked. Apart from `onChange` because it moves the port with it. */
  onKindChange: (kind: DbKind) => void;
  /** The id of the saved connection being edited, or null for one that is not saved yet. What
   *  decides whether the button says Save or Update, and whether Save as new is offered. */
  editingId: string | null;
  name: string;
  onNameChange: (name: string) => void;
  /** Whether Save would do anything: a name is needed, and an edit that changes nothing is not a
   *  save. Worked out by the tab, which is what holds the saved list. */
  saveDisabled: boolean;
  /** What the tab is saying about the connection attempt, beside the Connect button. */
  status: string;
  connecting: boolean;
  tunnelStatus: { tone: string; message: string } | null;
  /** Bumped each time the tab wants the caret in the password field — a handed-over connection
   *  that arrived with everything but the password. A counter rather than a flag, so asking twice
   *  moves it twice. `0`, or absent, asks nothing. */
  focusPassword?: number;
  /** The tab's banners — a failed connection, a saved entry changed elsewhere. Drawn right under
   *  the header, beside the Connect button that caused them, rather than below every card where
   *  nobody who just pressed it would see them. */
  notices?: ReactNode;
  onSave: () => void;
  onSaveAsNew: () => void;
  onConnect: () => void;
  onTestTunnel: () => void;
}

function ConnectionForm({
  form,
  onChange: set,
  onKindChange: changeKind,
  editingId,
  name: saveAsName,
  onNameChange: setSaveAsName,
  saveDisabled,
  status,
  connecting,
  tunnelStatus,
  focusPassword = 0,
  notices,
  onSave: saveConnection,
  onSaveAsNew: saveConnectionAsNew,
  onConnect: connect,
  onTestTunnel: testTunnel,
}: Props) {
  const { t } = useTranslation();
  const passwordRef = useRef<HTMLInputElement>(null);
  const engineTrigger = useRef<HTMLButtonElement>(null);
  const [pickingEngine, setPickingEngine] = useState(false);
  const [passwordShown, setPasswordShown] = useState(false);
  const [copied, setCopied] = useState(false);

  /** What went wrong making a database file, shown under the field. Cleared by the next attempt. */
  const [fileError, setFileError] = useState("");
  useEffect(() => {
    if (focusPassword > 0) passwordRef.current?.focus();
  }, [focusPassword]);
  const {
    kind, host, port, username, password, database, uri, path, uriRevealed, confirmingReveal, useSsl,
    tunnelType, sshHost, sshPort, sshUser, sshAuthType, sshPassword, sshKeyPath, sshPassphrase,
  } = form;

  async function browseForPrivateKey() {
    const path = await open({
      title: t("connection.selectPrivateKeyDialogTitle"),
      multiple: false,
      directory: false,
    });
    if (typeof path === "string") {
      set("sshKeyPath", path);
    }
  }

  /** The extensions a SQLite file is usually given, and an "any file" entry after them: the
   *  extension is a convention, not a format — plenty of applications keep theirs with no suffix at
   *  all, or with one of their own. */
  function sqliteFilters() {
    return [
      { name: "SQLite", extensions: ["db", "sqlite", "sqlite3", "db3"] },
      { name: t("connection.allFilesFilter"), extensions: ["*"] },
    ];
  }

  async function browseForDatabaseFile() {
    const chosen = await open({
      title: t("connection.selectSqliteFileDialogTitle"),
      multiple: false,
      directory: false,
      filters: sqliteFilters(),
    });
    if (typeof chosen === "string") {
      setFileError("");
      set("path", chosen);
    }
  }

  /**
   * Makes an empty database file and puts it in the box, ready to connect to.
   *
   * Two steps rather than one, and deliberately: connecting never creates a file, so that a
   * mistyped path is reported as the typo it is instead of opening an empty database nobody asked
   * for. Creating one is this button and nothing else.
   */
  async function createDatabaseFile() {
    const chosen = await save({
      title: t("connection.newSqliteFileDialogTitle"),
      defaultPath: "database.db",
      filters: sqliteFilters(),
    });
    if (typeof chosen !== "string") return;
    try {
      await createSqliteFile(chosen);
      set("path", chosen);
      setFileError("");
    } catch (e) {
      // Beside the field rather than in the tab's banner: the banner is for a failed connection,
      // and nothing has been connected to yet.
      setFileError(errorMessage(t, e));
    }
  }

  async function copyConnectionString() {
    await copyText(connectionString(form));
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  }

  const isMongo = kind === "mongo";
  const isSqlite = kind === "sqlite";
  /* Not `isSqlKind`: SQLite is one, and has no transport to secure. */
  const hasSsl = kind === "mysql" || kind === "postgres" || kind === "clickhouse" || kind === "mssql";
  const tunnelled = !isSqlite && tunnelType === "ssh";

  /* A Redis server whose default user has no password runs in protected mode unless it was told
     otherwise, and protected mode answers anything that isn't loopback with `-DENIED` and hangs
     up. The client only finds out when its next write hits the closed socket, so the tab reports
     a broken pipe and says nothing about why — hence the warning up here, where it can still be
     acted on.
     The tunnel doesn't come into it: this host is the address whoever dials Redis uses — this
     machine directly, or the SSH server on its behalf — so a loopback address means Redis is on
     the dialling machine either way, and anything else means it is not. */
  const showRedisProtectedModeHint =
    kind === "redis" && password === "" && !isLoopback(host);

  /**
   * Whether the SSH fields hold enough for a test to mean anything. Everything the chosen auth
   * method needs and nothing it doesn't: a passphrase belongs to a key that was encrypted, and an
   * unencrypted one has none, so it is never required. A test without these would only come back
   * with the server's own complaint about a missing host or user, one round trip later.
   */
  const sshInputsComplete =
    sshHost.trim() !== "" &&
    sshUser.trim() !== "" &&
    sshPort > 0 &&
    (sshAuthType === "password" ? sshPassword !== "" : sshKeyPath.trim() !== "");

  const place = connectionPlace({ kind, host, port, path, uri });
  const sshPlace = `${sshHost.trim() || "—"}${sshPort > 0 ? `:${sshPort}` : ""}`;

  return (
    <div className="connection-editor">
      <header className="editor-header">
        <EngineBadge kind={kind} size={52} />
        <div className="editor-heading">
          <h2 className="editor-title">{saveAsName.trim() || t("connection.untitled")}</h2>
          <div className="editor-state">
            <StatusPill tone={connecting ? "warning" : "neutral"} pulse={connecting}>
              {connecting ? t("connection.connecting") : t("connection.notConnected")}
            </StatusPill>
            {status !== "" && !connecting && <span className="editor-status">{status}</span>}
          </div>
        </div>
        <div className="editor-actions">
          <Button size="large" onClick={saveConnection} disabled={saveDisabled}>
            {editingId ? t("connection.updateConnection") : t("connection.saveConnection")}
          </Button>
          {editingId && (
            <Button size="large" onClick={saveConnectionAsNew} disabled={!saveAsName.trim()}>
              {t("connection.saveAsNew")}
            </Button>
          )}
          <Button size="large" variant="primary" onClick={() => connect()} disabled={connecting}>
            {t("common.connect")}
          </Button>
        </div>
      </header>

      {notices}

      <div className="editor-grid">
        <div className="editor-column">
          <Card title={t("connection.general")}>
            <div className="editor-fields">
              <label className="editor-field span-6">
                {editingId ? t("connection.nameLabel") : t("connection.saveAsLabel")}
                <Input
                  value={saveAsName}
                  onChange={(e) => setSaveAsName(e.target.value)}
                  placeholder={t("connection.connectionNamePlaceholder")}
                />
              </label>
              <div className="editor-field span-6">
                <span id="connection-engine-label">{t("connection.databaseLegend")}</span>
                <div className="engine-picker">
                  <button
                    ref={engineTrigger}
                    type="button"
                    className="engine-trigger"
                    aria-labelledby="connection-engine-label"
                    aria-haspopup="dialog"
                    aria-expanded={pickingEngine}
                    onClick={() => setPickingEngine((open) => !open)}
                  >
                    <EngineBadge kind={kind} size={38} />
                    <span className="engine-trigger-text">
                      <strong>{t(kindLabel(kind))}</strong>
                      <span>{t(engineDescription(kind))}</span>
                    </span>
                    <span className="engine-trigger-change">{t("connection.changeEngine")}</span>
                  </button>
                  <Popover
                    open={pickingEngine}
                    onClose={() => setPickingEngine(false)}
                    anchorRef={engineTrigger}
                    label={t("connection.databaseLegend")}
                    className="engine-options"
                  >
                    {ENGINES.map((k) => (
                      <button
                        key={k}
                        type="button"
                        className="engine-option"
                        aria-pressed={kind === k}
                        onClick={() => {
                          changeKind(k);
                          setPickingEngine(false);
                          engineTrigger.current?.focus();
                        }}
                      >
                        <EngineBadge kind={k} size={30} />
                        <span className="engine-option-text">
                          <strong>{t(kindLabel(k))}</strong>
                          <span>{t(engineDescription(k))}</span>
                        </span>
                      </button>
                    ))}
                  </Popover>
                </div>
              </div>
            </div>
          </Card>

          <Card title={isSqlite ? t("connection.file") : t("connection.server")}>
            <div className="editor-fields">
              {isSqlite ? (
                /* A path and nothing else. There is no host to reach, no account to be on and no
                   database to pick inside the file — the file is the database. */
                <label className="editor-field span-6">
                  {t("connection.sqlitePathLabel")}
                  <span className="editor-inline">
                    <Input
                      mono
                      value={path}
                      onChange={(e) => {
                        // The complaint was about the path that was there; a different one is a
                        // different question, and the answer to it comes from Connect.
                        setFileError("");
                        set("path", e.target.value);
                      }}
                      placeholder={t("connection.sqlitePathPlaceholder")}
                    />
                    <Button onClick={browseForDatabaseFile}>{t("common.browse")}</Button>
                    <Button onClick={createDatabaseFile}>{t("connection.newSqliteFile")}</Button>
                  </span>
                </label>
              ) : isMongo ? (
                <label className="editor-field span-6">
                  {t("connection.connectionStringLabel")}
                  <span className="editor-inline">
                    <Input
                      mono
                      value={uriRevealed ? uri : maskMongoUri(uri)}
                      onChange={(e) => set("uri", e.target.value)}
                      placeholder={t("connection.connectionStringPlaceholder")}
                      readOnly={!uriRevealed}
                    />
                    <Button
                      className="reveal-toggle"
                      aria-pressed={uriRevealed}
                      title={uriRevealed ? t("connection.hideConnectionString") : t("connection.revealConnectionString")}
                      aria-label={uriRevealed ? t("connection.hideConnectionString") : t("connection.revealConnectionString")}
                      onClick={() => (uriRevealed ? set("uriRevealed", false) : set("confirmingReveal", true))}
                    >
                      {/* The struck-through eye marks the state the button moves *to*: shown now,
                          click to hide. */}
                      {uriRevealed ? <EyeOffIcon size={16} /> : <EyeIcon size={16} />}
                    </Button>
                  </span>
                </label>
              ) : (
                <>
                  <label className="editor-field span-4">
                    {t("common.host")}
                    <Input mono value={host} onChange={(e) => set("host", e.target.value)} />
                  </label>
                  <label className="editor-field span-2">
                    {t("common.port")}
                    <Input
                      mono
                      type="number"
                      value={port}
                      onChange={(e) => set("port", Number(e.target.value))}
                    />
                  </label>
                  <label className="editor-field span-3">
                    {t("common.user")}
                    <Input value={username} onChange={(e) => set("username", e.target.value)} />
                  </label>
                  <label className="editor-field span-3">
                    {t("common.password")}
                    <span className="editor-inline">
                      <Input
                        ref={passwordRef}
                        type={passwordShown ? "text" : "password"}
                        value={password}
                        placeholder={form.keyringRef ? t("connection.passwordFromMixEngine") : undefined}
                        onChange={(e) => set("password", e.target.value)}
                        autoComplete="new-password"
                      />
                      <Button
                        className="reveal-toggle"
                        aria-pressed={passwordShown}
                        title={passwordShown ? t("connection.hidePassword") : t("connection.showPassword")}
                        aria-label={passwordShown ? t("connection.hidePassword") : t("connection.showPassword")}
                        onClick={() => setPasswordShown((shown) => !shown)}
                      >
                        {passwordShown ? <EyeOffIcon size={16} /> : <EyeIcon size={16} />}
                      </Button>
                    </span>
                  </label>
                  <label className="editor-field span-6">
                    {kind === "redis" ? t("connection.dbIndexLabel") : t("common.database")}
                    <Input mono value={database} onChange={(e) => set("database", e.target.value)} />
                  </label>
                </>
              )}
            </div>

            {/* Under the field it is about. Amber like the Redis hint below: nothing has been
                connected to yet, so this is a note about the box above, not a failed connection. */}
            {fileError !== "" && (
              <p className="field-warning" role="alert">
                {fileError}
              </p>
            )}

            {showRedisProtectedModeHint && (
              <p className="field-warning" role="status">
                {t("connection.redisNoPasswordWarning")}
              </p>
            )}

            {hasSsl && (
              <div className="switch-panel">
                <span className="switch-panel-text">
                  <span id="connection-use-ssl">{t("connection.useSslLabel")}</span>
                  <span className="switch-panel-hint">{t("connection.useSslHint")}</span>
                </span>
                <Switch
                  aria-labelledby="connection-use-ssl"
                  checked={useSsl}
                  onChange={(next) => set("useSsl", next)}
                />
              </div>
            )}
          </Card>
        </div>

        <div className="editor-column">
          <Card title={t("connection.connectionMethodLegend")}>
            {isSqlite ? (
              /* There is nothing to tunnel to: the file is on this machine. A note rather than a
                 disabled control, because a disabled control still says the choice exists. */
              <p className="editor-note">{t("connection.sqliteOpenedDirectly")}</p>
            ) : (
              <>
                <SegmentedControl
                  block
                  mode="tabs"
                  aria-label={t("connection.connectionMethodLegend")}
                  value={tunnelType}
                  onChange={(value) => set("tunnelType", value)}
                  segments={[
                    { value: "direct", label: t("connection.methodTcpIp") },
                    { value: "ssh", label: t("connection.methodSsh") },
                  ]}
                />
                {tunnelType === "ssh" && (
                  <div className="editor-fields editor-fields-spaced">
                    <label className="editor-field span-4">
                      {t("connection.sshHost")}
                      <Input mono value={sshHost} onChange={(e) => set("sshHost", e.target.value)} />
                    </label>
                    <label className="editor-field span-2">
                      {t("connection.sshPort")}
                      <Input
                        mono
                        type="number"
                        value={sshPort}
                        onChange={(e) => set("sshPort", Number(e.target.value))}
                      />
                    </label>
                    <label className="editor-field span-3">
                      {t("connection.sshUser")}
                      <Input value={sshUser} onChange={(e) => set("sshUser", e.target.value)} />
                    </label>
                    <label className="editor-field span-3">
                      {t("connection.auth")}
                      <Select
                        value={sshAuthType}
                        onChange={(v) => set("sshAuthType", v)}
                        options={[
                          {
                            value: "password",
                            label: t("connection.authPassword"),
                            optionLabel: (
                              <span className="option-with-hint">
                                {t("connection.authPassword")}
                                <span>{t("connection.authPasswordHint")}</span>
                              </span>
                            ),
                            searchText: t("connection.authPassword"),
                          },
                          {
                            value: "privatekey",
                            label: t("connection.authPrivateKey"),
                            optionLabel: (
                              <span className="option-with-hint">
                                {t("connection.authPrivateKey")}
                                <span>{t("connection.authPrivateKeyHint")}</span>
                              </span>
                            ),
                            searchText: t("connection.authPrivateKey"),
                          },
                        ]}
                      />
                    </label>
                    {sshAuthType === "password" ? (
                      <label className="editor-field span-6">
                        {t("connection.sshPassword")}
                        <Input
                          type="password"
                          value={sshPassword}
                          onChange={(e) => set("sshPassword", e.target.value)}
                          autoComplete="new-password"
                        />
                      </label>
                    ) : (
                      <>
                        <label className="editor-field span-6">
                          {t("connection.privateKeyFile")}
                          <span className="editor-inline">
                            <Input
                              mono
                              value={sshKeyPath}
                              onChange={(e) => set("sshKeyPath", e.target.value)}
                              placeholder={PRIVATE_KEY_PLACEHOLDER}
                            />
                            <Button onClick={browseForPrivateKey}>{t("common.browse")}</Button>
                          </span>
                        </label>
                        <label className="editor-field span-6">
                          {t("connection.keyPassphrase")}
                          <Input
                            type="password"
                            value={sshPassphrase}
                            onChange={(e) => set("sshPassphrase", e.target.value)}
                            placeholder={t("connection.passphrasePlaceholder")}
                            autoComplete="new-password"
                          />
                        </label>
                      </>
                    )}
                    <div className="editor-tunnel span-6">
                      {tunnelStatus && (
                        <span className={`tunnel-status tunnel-status-${tunnelStatus.tone}`}>
                          {tunnelStatus.message}
                        </span>
                      )}
                      <Button variant="soft" onClick={testTunnel} disabled={!sshInputsComplete}>
                        {t("connection.testTunnel")}
                      </Button>
                    </div>
                  </div>
                )}
              </>
            )}
          </Card>

          <Card title={t("connection.route")}>
            {/* The way a connection travels, drawn: this computer, the SSH server when there is one,
                and the engine at the address the form will dial. */}
            <ol className="route" aria-label={t("connection.route")}>
              <li className="route-node">
                <span className="route-icon">
                  <MonitorIcon size={18} />
                </span>
                <span className="route-name">{t("connection.thisComputer")}</span>
              </li>
              {isSqlite ? (
                <li className="route-link route-link-file">{t("connection.routeFile")}</li>
              ) : tunnelled ? (
                <>
                  <li className="route-link route-link-encrypted">{t("connection.routeEncrypted")}</li>
                  <li className="route-node">
                    <span className="route-icon">
                      <ServerIcon size={18} />
                    </span>
                    <span className="route-name">{t("connection.sshServer")}</span>
                    <span className="route-address">{sshPlace}</span>
                  </li>
                  <li className="route-link">{t("connection.routeLocal")}</li>
                </>
              ) : (
                <li className="route-link">{t("connection.routeDirect")}</li>
              )}
              <li className="route-node">
                <EngineBadge kind={kind} size={38} />
                <span className="route-name">{t(kindLabel(kind))}</span>
                <span className="route-address">{place || "—"}</span>
              </li>
            </ol>

            <div className="connection-string">
              <span className="connection-string-label">{t("connection.connectionStringLabel")}</span>
              <code>{connectionString(form) || "—"}</code>
              <Button
                size="small"
                variant="ghost"
                aria-label={t("connection.copyConnectionString")}
                title={copied ? t("connection.copied") : t("connection.copyConnectionString")}
                onClick={() => void copyConnectionString()}
              >
                <CopyIcon size={14} />
              </Button>
            </div>
          </Card>
        </div>
      </div>

      {confirmingReveal && (
        <ConfirmDialog
          title={t("connection.revealConnectionStringTitle")}
          message={t("connection.revealConnectionStringMessage")}
          confirmLabel={t("connection.revealConnectionStringConfirm")}
          onConfirm={() => {
            set("uriRevealed", true);
            set("confirmingReveal", false);
          }}
          onCancel={() => set("confirmingReveal", false)}
        />
      )}
    </div>
  );
}

export default ConnectionForm;
