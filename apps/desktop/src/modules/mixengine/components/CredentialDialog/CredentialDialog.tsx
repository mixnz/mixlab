import { useState } from "react";

import Input from "../../../../components/Input";
import Modal, { ModalBody } from "../../../../components/Modal";
import { copyText } from "../../../../core/clipboard";
import { useTranslation } from "../../../../i18n";
import type { DatabaseCredentials } from "@mixengine/api";
import styles from "./CredentialDialog.module.css";

/**
 * The password MixEngine holds for one account, enough to paste into a project's `.env`.
 *
 * **Hidden until asked for.** This window opens from a menu, so it may open in front of someone
 * else looking at the screen; a password shown up front gives nobody the chance to decide that.
 * The Copy button does not need to see it, so that is the default path and it comes first.
 *
 * The address in the credential store comes along because it answers a different question: **where
 * this lives** when someone wants to change it with the operating system's tools. `SecretAddress`
 * carries both halves (T84), so this can be drawn without knowing MixEngine's namespace.
 */
export default function CredentialDialog({
  credentials,
  onClose,
}: {
  credentials: DatabaseCredentials;
  onClose: () => void;
}) {
  const [shown, setShown] = useState(false);
  const [copied, setCopied] = useState(false);
  const { t } = useTranslation();

  const title = t("mixengine.credentials.title", { service: credentials.service });

  return (
    <Modal
      title={title}
      onClose={onClose}
      size="small"
      footerNote={copied ? t("mixengine.credentials.copied") : undefined}
      actions={[
        {
          kind: "secondary",
          label: t("mixengine.credentials.copy"),
          onClick: () => {
            void copyText(credentials.password);
            setCopied(true);
          },
        },
        {
          kind: "secondary",
          label: t(shown ? "mixengine.credentials.hide" : "mixengine.credentials.show"),
          onClick: () => setShown((was) => !was),
        },
        { kind: "cancel", label: t("mixengine.credentials.close") },
      ]}
    >
      {() => (
        <>
          <ModalBody>
            <p className={styles.line}>
              {t("mixengine.credentials.account", { user: credentials.user })}
            </p>

            <label className={styles.field}>
              {t("mixengine.credentials.password")}
              {/* `readOnly` rather than `disabled`: a greyed-out field does not let you select its
                  text, and selecting by hand is the fallback when the webview refuses the
                  clipboard (see `core/clipboard.ts`). */}
              <Input
                value={credentials.password}
                type={shown ? "text" : "password"}
                readOnly
                onFocus={(e) => e.currentTarget.select()}
              />
            </label>

            <p className={styles.hint}>
              {t("mixengine.credentials.storedAt", { key: credentials.secret.key })}
            </p>
          </ModalBody>
        </>
      )}
    </Modal>
  );
}
