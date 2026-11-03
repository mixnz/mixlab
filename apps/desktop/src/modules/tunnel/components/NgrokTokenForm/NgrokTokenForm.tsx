import { useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";

import Button from "../../../../components/Button";
import Input from "../../../../components/Input";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import { readProgramPath } from "../../settings";
import styles from "./NgrokTokenForm.module.css";

/** Where ngrok shows a person their authtoken. */
export const NGROK_AUTHTOKEN_PAGE = "https://dashboard.ngrok.com/get-started/your-authtoken";

interface Props {
  /** The field's id, for a label outside the form. */
  id?: string;
  disabled?: boolean;
  /** Offer the dashboard page beside Save. */
  showGetLink?: boolean;
  onSaved?: () => void;
}

/**
 * A field that hands an authtoken to ngrok. The token lives in this component's state until Save,
 * which asks ngrok to write it into ngrok's own config, then the field is emptied: nothing of it
 * is stored by MixLab. Saving also starts again every ngrok tunnel that failed for want of one.
 */
export default function NgrokTokenForm({ id, disabled = false, showGetLink = false, onSaved }: Props) {
  const { t } = useTranslation();
  const [token, setToken] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  async function save() {
    setError("");
    setSaving(true);
    try {
      await api.tunnelNgrokAuthtoken(token, readProgramPath("ngrok"));
      setToken("");
      onSaved?.();
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className={styles.form}>
      <form
        className={styles.row}
        onSubmit={(event) => {
          event.preventDefault();
          void save();
        }}
      >
        <Input
          id={id}
          type="password"
          aria-label={t("tunnelTab.ngrokAuthtokenLabel")}
          placeholder={t("tunnelTab.ngrokAuthtokenPlaceholder")}
          value={token}
          mono
          disabled={disabled}
          onChange={(event) => setToken(event.target.value)}
        />
        <Button
          type="submit"
          variant="primary"
          disabled={disabled || token.trim() === ""}
          busy={saving ? t("tunnelTab.saveAuthtoken") : undefined}
        >
          {t("tunnelTab.saveAuthtoken")}
        </Button>
        {showGetLink && (
          <Button variant="soft" onClick={() => void openUrl(NGROK_AUTHTOKEN_PAGE)}>
            {t("tunnelTab.openNgrokDashboard")}
          </Button>
        )}
      </form>
      {error !== "" && <p className={styles.error}>{error}</p>}
    </div>
  );
}
