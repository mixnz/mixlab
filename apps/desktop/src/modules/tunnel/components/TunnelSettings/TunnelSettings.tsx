import { useEffect, useState } from "react";

import Button from "../../../../components/Button";
import Input from "../../../../components/Input";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import { readProgramPath, writeProgramPath } from "../../settings";
import type { Provider } from "../../tunnelState";
import styles from "./TunnelSettings.module.css";

/** One program's path: empty means the one on PATH, or the one MixLab downloads. */
function ProgramPath({ provider, label, onChange }: { provider: Provider; label: string; onChange?: () => void }) {
  const { t } = useTranslation();
  const [path, setPath] = useState(() => readProgramPath(provider));
  const id = `tunnel-${provider}-path`;

  return (
    <div className={styles.section}>
      <label className={styles.label} htmlFor={id}>
        {label}
      </label>
      <Input
        id={id}
        value={path}
        mono
        allowClear
        onChange={(event) => {
          setPath(event.target.value);
          writeProgramPath(provider, event.target.value);
          onChange?.();
        }}
      />
      <p className={styles.hint}>{t("tunnelTab.pathHint")}</p>
    </div>
  );
}

/**
 * The Tunnel pane in Settings: which `cloudflared` and which `ngrok` to run, and ngrok's authtoken.
 *
 * **The authtoken is handed to ngrok and forgotten.** It lives in this component's state until Save,
 * which asks ngrok to write it into ngrok's own config; the field is then emptied, and nothing of it
 * is stored by MixLab.
 */
export default function TunnelSettings() {
  const { t } = useTranslation();
  const [token, setToken] = useState("");
  const [hasNgrok, setHasNgrok] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState("");
  const [pathChanged, setPathChanged] = useState(0);

  useEffect(() => {
    api.tunnelBinary("ngrok", readProgramPath("ngrok")).then(
      (status) => setHasNgrok(status.found !== null),
      () => setHasNgrok(false),
    );
  }, [pathChanged]);

  async function save() {
    setError("");
    setSaved(false);
    setSaving(true);
    try {
      await api.tunnelNgrokAuthtoken(token, readProgramPath("ngrok"));
      setToken("");
      setSaved(true);
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setSaving(false);
    }
  }

  return (
    <>
      <ProgramPath provider="cloudflare" label={t("tunnelTab.pathLabel")} />
      <ProgramPath provider="ngrok" label={t("tunnelTab.ngrokPathLabel")} onChange={() => setPathChanged((n) => n + 1)} />
      <div className={styles.section}>
        <label className={styles.label} htmlFor="tunnel-ngrok-authtoken">
          {t("tunnelTab.ngrokAuthtokenLabel")}
        </label>
        <form
          className={styles.row}
          onSubmit={(event) => {
            event.preventDefault();
            void save();
          }}
        >
          <Input
            id="tunnel-ngrok-authtoken"
            type="password"
            value={token}
            mono
            disabled={!hasNgrok}
            onChange={(event) => {
              setToken(event.target.value);
              setSaved(false);
            }}
          />
          <Button
            type="submit"
            disabled={!hasNgrok || token.trim() === ""}
            busy={saving ? t("tunnelTab.saveAuthtoken") : undefined}
          >
            {t("tunnelTab.saveAuthtoken")}
          </Button>
        </form>
        <p className={styles.hint}>
          {!hasNgrok
            ? t("tunnelTab.ngrokAuthtokenNeedsNgrok")
            : saved
              ? t("tunnelTab.authtokenSaved")
              : t("tunnelTab.ngrokAuthtokenHint")}
        </p>
        {error !== "" && <p className={styles.error}>{error}</p>}
      </div>
    </>
  );
}
