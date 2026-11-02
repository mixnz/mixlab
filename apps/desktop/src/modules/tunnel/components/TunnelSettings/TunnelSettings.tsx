import { useCallback, useEffect, useState } from "react";

import Button from "../../../../components/Button";
import ConfirmDialog from "../../../../components/ConfirmDialog";
import Input from "../../../../components/Input";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import { readProgramPath, writeProgramPath } from "../../settings";
import { tokenStateKey, type AuthtokenStatus, type Provider } from "../../tunnelState";
import NgrokTokenForm from "../NgrokTokenForm";
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
 * **The authtoken is ngrok's.** This pane says whether one is set and where from, never what it is;
 * it hands a new one to ngrok, and Remove empties the one in ngrok's config. A token from
 * `NGROK_AUTHTOKEN` cannot be removed from here, and the pane says where it comes from instead.
 */
export default function TunnelSettings() {
  const { t } = useTranslation();
  const [hasNgrok, setHasNgrok] = useState(false);
  const [status, setStatus] = useState<AuthtokenStatus | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [error, setError] = useState("");

  const read = useCallback(() => {
    const configured = readProgramPath("ngrok");
    api.tunnelBinary("ngrok", configured).then(
      (binary) => setHasNgrok(binary.found !== null),
      () => setHasNgrok(false),
    );
    api.tunnelNgrokAuthtokenStatus(configured).then(setStatus, () => setStatus(null));
  }, []);

  useEffect(() => {
    read();
    const changed = api.onChanged(read);
    return () => {
      void changed.then((unlisten) => unlisten());
    };
  }, [read]);

  async function remove() {
    setConfirming(false);
    setError("");
    try {
      await api.tunnelNgrokAuthtokenRemove(readProgramPath("ngrok"));
      read();
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }

  return (
    <>
      <ProgramPath provider="cloudflare" label={t("tunnelTab.pathLabel")} />
      <ProgramPath provider="ngrok" label={t("tunnelTab.ngrokPathLabel")} onChange={read} />
      <div className={styles.section}>
        <label className={styles.label} htmlFor="tunnel-ngrok-authtoken">
          {t("tunnelTab.ngrokAuthtokenLabel")}
        </label>
        {status && (
          <div className={styles.state}>
            <span className={status.set ? styles.set : styles.unset}>
              {t(tokenStateKey(status), { path: status.path ?? "" })}
            </span>
            {status.source === "config" && hasNgrok && (
              <Button size="small" variant="danger" onClick={() => setConfirming(true)}>
                {t("tunnelTab.removeAuthtoken")}
              </Button>
            )}
          </div>
        )}
        <NgrokTokenForm id="tunnel-ngrok-authtoken" disabled={!hasNgrok} onSaved={read} />
        <p className={styles.hint}>
          {hasNgrok ? t("tunnelTab.ngrokAuthtokenHint") : t("tunnelTab.ngrokAuthtokenNeedsNgrok")}
        </p>
        {error !== "" && <p className={styles.error}>{error}</p>}
      </div>
      {confirming && (
        <ConfirmDialog
          title={t("tunnelTab.removeAuthtokenTitle")}
          message={t("tunnelTab.removeAuthtokenMessage")}
          confirmLabel={t("tunnelTab.removeAuthtoken")}
          danger
          onConfirm={() => void remove()}
          onCancel={() => setConfirming(false)}
        />
      )}
    </>
  );
}
