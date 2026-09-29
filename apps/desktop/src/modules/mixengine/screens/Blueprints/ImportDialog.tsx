import { useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import Button from "../../../../components/Button";
import Input from "../../../../components/Input";
import Modal, { ModalBody, ModalErrors } from "../../../../components/Modal";
import Checkbox from "../../../../components/Checkbox";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import styles from "./ImportDialog.module.css";

interface Props {
  onCancel: () => void;
  onImported: () => void;
}

/** Imports a blueprint from a `.toml` file. Never fails because of the signature — the result only
 *  changes `trusted`/`signature` on the returned `BlueprintSummary`, and the list shows that by
 *  itself. */
export default function ImportDialog({ onCancel, onImported }: Props) {
  const { t } = useTranslation();
  const [path, setPath] = useState("");
  const [name, setName] = useState("");
  const [overwrite, setOverwrite] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  async function browse() {
    const picked = await openDialog({
      multiple: false,
      filters: [{ name: "Blueprint", extensions: ["toml"] }],
    });
    if (typeof picked === "string") setPath(picked);
  }

  async function submit() {
    setSaving(true);
    setError("");
    try {
      await api.blueprintImport({
        path,
        name: name.trim() === "" ? undefined : name,
        overwrite,
      });
      onImported();
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setSaving(false);
    }
  }

  return (
    <Modal
      title={t("mixengine.blueprints.import.title")}
      onClose={onCancel}
      locked={saving}
      size="small"
      actions={[
        { kind: "cancel", label: t("common.cancel"), disabled: saving },
        {
          kind: "confirm",
          label: t("common.save"),
          onClick: () => void submit(),
          disabled: path.trim() === "",
          busy: saving ? t("mixengine.blueprints.import.saving") : undefined,
        },
      ]}
    >
      {() => (
        <>
          <ModalBody>
            <label className={styles.field}>
              {t("mixengine.blueprints.import.path")}
              <div className={styles.pathRow}>
                <Input value={path} disabled={saving} onChange={(e) => setPath(e.target.value)} />
                <Button onClick={() => void browse()} disabled={saving}>
                  {t("common.browse")}
                </Button>
              </div>
              <p className={styles.hint}>{t("mixengine.blueprints.import.signatureHint")}</p>
            </label>

            <label className={styles.field}>
              {t("mixengine.blueprints.import.name")}
              <Input
                value={name}
                disabled={saving}
                onChange={(e) => setName(e.target.value)}
                placeholder={t("mixengine.blueprints.import.namePlaceholder")}
              />
            </label>

            <Checkbox
              className={styles.checkbox}
              label={t("mixengine.blueprints.import.overwrite")}
              checked={overwrite}
              disabled={saving}
              onChange={(e) => setOverwrite(e.target.checked)}
            />
          </ModalBody>
          <ModalErrors messages={[error]} />
        </>
      )}
    </Modal>
  );
}
