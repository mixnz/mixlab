import { useMemo, useState } from "react";
import { Textarea } from "../../../../components/Input";
import JsonView from "../../../../components/JsonView";
import { useTranslation } from "../../../../i18n";
import CopyField from "../../components/CopyField";
import { toOutputs } from "../timestamp/time";
import { claimTimes, decodeJwt } from "./jwt";
import styles from "./Panel.module.css";

const REASON_KEY = {
  shape: "toolbox.jwt.badShape",
  base64: "toolbox.jwt.badBase64",
  json: "toolbox.jwt.badJson",
} as const;

const LOCAL_ZONE = Intl.DateTimeFormat().resolvedOptions().timeZone;

function JwtPanel() {
  const { t } = useTranslation();
  const [token, setToken] = useState("");
  // Frozen at mount: if "expired or not" changed by itself midway, the reader could not tell
  // whether it just changed or they misread it. Pasting a new token is enough to ask again.
  const [now] = useState(() => Date.now());

  const result = useMemo(() => (token.trim() === "" ? null : decodeJwt(token)), [token]);
  const times = useMemo(
    () => (result?.ok ? claimTimes(result.parts.payload, now) : null),
    [result, now],
  );

  const readable = (seconds: number) => toOutputs(seconds * 1000, LOCAL_ZONE, now).isoLocal;

  return (
    <div className={styles.panel}>
      <label className={styles.field}>
        <span className={styles.fieldLabel}>{t("toolbox.jwt.token")}</span>
        <Textarea
          value={token}
          onChange={(event) => setToken(event.target.value)}
          placeholder="eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.…"
          maxRows={6}
        />
      </label>

      {result && !result.ok ? <p className={styles.bad}>{t(REASON_KEY[result.reason])}</p> : null}

      {result?.ok ? (
        <>
          <section className={styles.section}>
            <h3 className={styles.heading}>{t("toolbox.jwt.header")}</h3>
            <JsonView value={result.parts.header} />
          </section>

          <section className={styles.section}>
            <h3 className={styles.heading}>{t("toolbox.jwt.payload")}</h3>
            <JsonView value={result.parts.payload} />
          </section>

          {times ? (
            <section className={styles.section}>
              {times.expired === null ? null : (
                <p className={times.expired ? styles.expired : styles.valid}>
                  {times.expired ? t("toolbox.jwt.expired") : t("toolbox.jwt.valid")}
                </p>
              )}
              {times.exp === undefined ? null : (
                <CopyField label={t("toolbox.jwt.expiresAt")} value={readable(times.exp)} />
              )}
              {times.iat === undefined ? null : (
                <CopyField label={t("toolbox.jwt.issuedAt")} value={readable(times.iat)} />
              )}
              {times.nbf === undefined ? null : (
                <CopyField label={t("toolbox.jwt.notBefore")} value={readable(times.nbf)} />
              )}
            </section>
          ) : null}

          <section className={styles.section}>
            <h3 className={styles.heading}>{t("toolbox.jwt.signature")}</h3>
            <p className={styles.signature}>{result.parts.signature}</p>
            <p className={styles.notVerified}>{t("toolbox.jwt.notVerified")}</p>
          </section>
        </>
      ) : null}
    </div>
  );
}

export default JwtPanel;
