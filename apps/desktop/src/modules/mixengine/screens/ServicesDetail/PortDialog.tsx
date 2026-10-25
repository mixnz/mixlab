import { useEffect, useRef, useState, type ReactNode } from "react";

import type { ServiceSetResult } from "@mixengine/api";
import Input from "../../../../components/Input";
import Modal, { ModalBody, ModalErrors } from "../../../../components/Modal";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import { confirmLabel, parsePort } from "../../portDialogState";
import styles from "./PortDialog.module.css";

interface Props {
  service: string;
  /** The port the service holds now, which the field starts at. */
  port: number;
  /** Whether the move will restart it — said above the button, and on it. */
  running: boolean;
  onClose: () => void;
  /** Called once the daemon has moved it, so the caller can read its rows again. */
  onMoved?: (result: ServiceSetResult) => void;
  /** Drawn under the result, for what the caller can offer next. */
  afterMove?: (result: ServiceSetResult) => ReactNode;
}

/**
 * Moving a service to another port: the Dashboard row menu's *Change port…* and the Port panel's
 * *Change…* both open this.
 *
 * **The daemon decides, this asks.** A port the field cannot hold is answered under it before
 * anything is sent; everything else — held by another service, bound by a program MixEngine does
 * not manage — is the daemon's sentence, shown in the same place, with the dialog left open. The
 * running sentence and the button's label are the confirmation; there is no second dialog.
 */
export default function PortDialog({ service, port, running, onClose, onMoved, afterMove }: Props) {
  const { t } = useTranslation();
  const [text, setText] = useState(String(port));
  const [errors, setErrors] = useState<string[]>([]);
  const [saving, setSaving] = useState(false);
  const [result, setResult] = useState<ServiceSetResult | null>(null);
  const field = useRef<HTMLInputElement>(null);

  useEffect(() => {
    field.current?.focus();
    field.current?.select();
  }, []);

  async function submit() {
    const asked = parsePort(text);
    if (asked === null) {
      setErrors([t("mixengine.port.invalid")]);
      return;
    }
    setErrors([]);
    setSaving(true);
    try {
      const moved = await api.serviceSet(service, asked);
      setResult(moved);
      onMoved?.(moved);
    } catch (e) {
      setErrors([errorMessage(t, e)]);
    } finally {
      setSaving(false);
    }
  }

  if (result !== null) {
    const change = result.port;
    const summary =
      change === undefined || change === null || change.from === change.to
        ? t("mixengine.port.unchanged", { service, to: String(change?.to ?? port) })
        : t(result.restarted ? "mixengine.port.movedRestarted" : "mixengine.port.moved", {
            service,
            to: String(change.to),
          });

    return (
      <Modal
        title={t("mixengine.port.title", { service })}
        size="small"
        onClose={onClose}
        actions={[{ kind: "confirm", label: t("mixengine.port.done"), onClick: onClose }]}
      >
        {() => (
          <ModalBody>
            <p className={styles.sentence}>{summary}</p>
            {result.not_followed.length > 0 && change !== undefined && change !== null && (
              <>
                <p className={styles.note}>{t("mixengine.port.notFollowed", { from: String(change.from) })}</p>
                <ul className={styles.list}>
                  {result.not_followed.map((line) => (
                    <li key={line}>{line}</li>
                  ))}
                </ul>
              </>
            )}
            {afterMove?.(result)}
          </ModalBody>
        )}
      </Modal>
    );
  }

  const label = t(confirmLabel(running));

  return (
    <Modal
      title={t("mixengine.port.title", { service })}
      size="small"
      onClose={onClose}
      locked={saving}
      actions={[
        { kind: "cancel", label: t("common.cancel"), disabled: saving },
        { kind: "confirm", label, onClick: () => void submit(), busy: saving ? label : undefined },
      ]}
    >
      {() => (
        <>
          <ModalBody>
            {running && <p className={styles.note}>{t("mixengine.port.running", { service })}</p>}
            <label className={styles.field}>
              {t("mixengine.port.label")}
              <Input
                ref={field}
                mono
                inputMode="numeric"
                value={text}
                disabled={saving}
                onChange={(e) => setText(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !saving) void submit();
                }}
              />
            </label>
          </ModalBody>
          <ModalErrors messages={errors} />
        </>
      )}
    </Modal>
  );
}
