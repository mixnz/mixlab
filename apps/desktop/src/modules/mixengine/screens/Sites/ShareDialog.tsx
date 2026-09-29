import { useState } from "react";

import Input from "../../../../components/Input";
import Modal, { ModalBody, ModalErrors } from "../../../../components/Modal";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { SiteSharing } from "@mixengine/api";
import styles from "./ShareDialog.module.css";

interface Props {
  domain: string;
  onCancel: () => void;
  onShared: (sharing: SiteSharing) => void;
}

/**
 * Shares a site on the LAN.
 *
 * **Does not pick an interface by itself.** A machine with only one candidate is fine left empty.
 * On a machine with more than one, `site.share` refuses and names them in `hint` — a sentence, not
 * a structured list — so the interface field is always a hand-typed text field, not an auto-filled
 * dropdown. Read the `hint`, type it in, try again: the logic of choosing the right interface stays
 * on the daemon's side.
 */
export default function ShareDialog({ domain, onCancel, onShared }: Props) {
  const { t } = useTranslation();
  const [iface, setIface] = useState("");
  const [minutes, setMinutes] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  async function submit() {
    setSaving(true);
    setError("");
    try {
      const forSeconds = minutes.trim() === "" ? null : Number(minutes) * 60;
      const sharing = await api.siteShare({
        site: { domain },
        interface: iface.trim() === "" ? null : iface.trim(),
        for_seconds: forSeconds,
      });
      onShared(sharing);
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setSaving(false);
    }
  }

  return (
    <Modal
      title={t("mixengine.sites.share.title")}
      onClose={onCancel}
      locked={saving}
      size="small"
      actions={[
        { kind: "cancel", label: t("common.cancel"), disabled: saving },
        {
          kind: "confirm",
          label: t("mixengine.sites.share.title"),
          onClick: () => void submit(),
          busy: saving ? t("mixengine.sites.form.saving") : undefined,
        },
      ]}
    >
      {() => (
        <>
          <ModalBody>
            <label className={styles.field}>
              {t("mixengine.sites.share.interfaceLabel")}
              <Input
                value={iface}
                disabled={saving}
                onChange={(e) => setIface(e.target.value)}
                placeholder={t("mixengine.sites.share.interfaceHint")}
              />
            </label>

            <label className={styles.field}>
              {t("mixengine.sites.share.durationLabel")}
              <Input
                type="number"
                min={1}
                value={minutes}
                disabled={saving}
                onChange={(e) => setMinutes(e.target.value)}
                placeholder={t("mixengine.sites.share.noExpiry")}
              />
            </label>
          </ModalBody>
          <ModalErrors messages={[error]} />
        </>
      )}
    </Modal>
  );
}
