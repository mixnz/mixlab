import { useEffect, useState } from "react";
import Button from "../../../../components/Button";
import Input from "../../../../components/Input";
import NoticeBanner from "../../../../components/NoticeBanner";
import SegmentedControl from "../../../../components/SegmentedControl";
import Select from "../../../../components/Select";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import { syncLogin, syncRegister, syncServerClosing, type SyncStatus } from "../../../sync/api";
import { addServer, DEFAULT_SERVER, lastServer, readServers, rememberServer, removeServer } from "../../../sync/servers";
import settings from "../SettingsModal.module.css";
import Field from "./Field";
import PasswordPair from "./PasswordPair";
import styles from "./SyncSection.module.css";

type Mode = "signIn" | "signUp";

interface Props {
  deviceName: string;
  onDeviceNameChange: (name: string) => void;
  onSignedIn: (status: SyncStatus) => void;
  /** Sign-up reached the server: the recovery key, to be shown once. */
  onRegistered: (recoveryKey: string, email: string) => void;
  /** The password is forgotten: reset it on the server chosen here (D6). */
  onForgot: (server: string, access: string | null, email: string) => void;
}

/**
 * Signed out: which server, and either signing in or making an account. The password leaves this
 * form only as an argument to Rust, which stretches it and forgets it (D2); a closed server's
 * access token goes the same way and is kept beside `MK`, never in the list of servers.
 *
 * The access-token field is shown only for a server that is not the default: the default instance
 * is open, and a field nobody there needs is a field somebody fills with their password.
 */
function SignInForm({ deviceName, onDeviceNameChange, onSignedIn, onRegistered, onForgot }: Props) {
  const { t, lang } = useTranslation();
  const [mode, setMode] = useState<Mode>("signIn");
  const [servers, setServers] = useState(() => readServers(localStorage));
  const [server, setServerState] = useState(() => lastServer(localStorage));
  const setServer = (url: string) => {
    rememberServer(localStorage, url);
    setServerState(url);
  };
  /** The address being typed for a new server, or `null` when none is being added. */
  const [adding, setAdding] = useState<string | null>(null);
  const [access, setAccess] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [again, setAgain] = useState("");
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  /** The chosen server's announced end, asked before anybody signs in or registers there. */
  const [serverClosing, setServerClosing] = useState<number | null>(null);
  const token = server === DEFAULT_SERVER || access.trim() === "" ? null : access.trim();
  /** The token the closing date is asked with: settled when the field is left, not per keystroke.
   *  Every half-typed token was a wrong one, and a closed server counts wrong tokens per network. */
  const [settledToken, setSettledToken] = useState<string | null>(null);
  const askedToken = server === DEFAULT_SERVER ? null : settledToken;

  useEffect(() => {
    let current = true;
    setServerClosing(null);
    syncServerClosing(server, askedToken)
      .then((on) => {
        if (current) setServerClosing(on);
      })
      // A server that cannot be asked — unreachable, or closed until its token is typed — is a
      // question for signing in, which will say so in its own words.
      .catch(() => {});
    return () => {
      current = false;
    };
  }, [server, askedToken]);

  function add() {
    const result = addServer(localStorage, adding ?? "");
    if (!result.ok) {
      setProblem(t(result.reason === "insecure" ? "sync.serverInsecure" : "sync.serverInvalid"));
      return;
    }
    setServers(result.servers);
    setServer(result.url);
    setAdding(null);
    setProblem(null);
  }

  function remove() {
    setServers(removeServer(localStorage, server));
    setServer(DEFAULT_SERVER);
  }

  async function submit() {
    if (mode === "signUp" && password !== again) {
      setProblem(t("sync.passwordsDiffer"));
      return;
    }
    setProblem(null);
    setBusy(true);
    try {
      if (mode === "signIn") onSignedIn(await syncLogin(server, token, email.trim(), password, deviceName.trim()));
      else onRegistered(await syncRegister(server, token, email.trim(), password), email.trim());
    } catch (error) {
      setProblem(errorMessage(t, error));
    } finally {
      setBusy(false);
    }
  }

  const ready =
    email.trim() !== "" && password !== "" && deviceName.trim() !== "" && (mode === "signIn" || again !== "");

  return (
    <form
      className={settings.section}
      onSubmit={(event) => {
        event.preventDefault();
        void submit();
      }}
    >
      <p className={settings.hint}>{t("sync.intro")}</p>
      <SegmentedControl<Mode>
        aria-label={t("sync.title")}
        block
        value={mode}
        onChange={(next) => {
          setMode(next);
          setProblem(null);
        }}
        segments={[
          { value: "signIn", label: t("sync.signIn") },
          { value: "signUp", label: t("sync.signUp") },
        ]}
      />

      <div className={styles.field}>
        <span className={settings.sectionLabel}>{t("sync.server")}</span>
        <div className={styles.row}>
          <Select<string>
            className={styles.grow}
            value={server}
            ariaLabel={t("sync.server")}
            options={servers.map((url) => ({
              value: url,
              label: url === DEFAULT_SERVER ? t("sync.serverDefault", { url }) : url,
            }))}
            onChange={setServer}
          />
          {server !== DEFAULT_SERVER && (
            <Button size="small" variant="ghost" onClick={remove}>
              {t("sync.removeServer")}
            </Button>
          )}
          {adding === null && (
            <Button size="small" onClick={() => setAdding("")}>
              {t("sync.addServer")}
            </Button>
          )}
        </div>
        {adding !== null && (
          <div className={styles.row}>
            <Input
              className={styles.grow}
              mono
              value={adding}
              placeholder="https://sync.example.com"
              aria-label={t("sync.addServer")}
              onChange={(event) => setAdding(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  add();
                }
              }}
            />
            <Button size="small" variant="primary" onClick={add}>
              {t("sync.addServerAction")}
            </Button>
            <Button
              size="small"
              variant="ghost"
              onClick={() => setAdding(null)}
            >
              {t("common.cancel")}
            </Button>
          </div>
        )}
      </div>

      {serverClosing !== null && (
        <NoticeBanner
          message={t("sync.serverClosing", {
            server,
            date: new Date(serverClosing * 1000).toLocaleDateString(lang),
          })}
        />
      )}
      {server !== DEFAULT_SERVER && (
        <Field label={t("sync.access")} hint={t("sync.accessHint")}>
          <Input
            type="password"
            value={access}
            onChange={(event) => setAccess(event.target.value)}
            onBlur={() => setSettledToken(token)}
          />
        </Field>
      )}
      <Field label={t("sync.email")}>
        <Input type="email" value={email} onChange={(event) => setEmail(event.target.value)} />
      </Field>
      {mode === "signIn" ? (
        <Field label={t("common.password")}>
          <Input
            type="password"
            autoComplete="current-password"
            value={password}
            onChange={(event) => setPassword(event.target.value)}
          />
        </Field>
      ) : (
        <PasswordPair
          label={t("common.password")}
          hint={t("sync.passwordHint")}
          password={password}
          again={again}
          onPassword={setPassword}
          onAgain={setAgain}
        />
      )}
      <Field label={t("sync.deviceName")}>
        <Input value={deviceName} onChange={(event) => onDeviceNameChange(event.target.value)} />
      </Field>

      {problem && (
        <p className={styles.problem} role="alert">
          {problem}
        </p>
      )}
      <div className={styles.row}>
        {mode === "signIn" && (
          <Button
            variant="link"
            onClick={() =>
              onForgot(server, token, email.trim())
            }
          >
            {t("sync.forgot")}
          </Button>
        )}
        <Button
          variant="primary"
          type="submit"
          disabled={!ready}
          busy={busy ? t(mode === "signIn" ? "sync.signingIn" : "sync.signingUp") : undefined}
        >
          {t(mode === "signIn" ? "sync.signIn" : "sync.signUp")}
        </Button>
      </div>
    </form>
  );
}

export default SignInForm;
