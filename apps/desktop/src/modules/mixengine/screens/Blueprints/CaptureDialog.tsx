import { useEffect, useState } from "react";

import Input from "../../../../components/Input";
import Modal, { ModalBody, ModalErrors } from "../../../../components/Modal";
import Select from "../../../../components/Select";
import Checkbox from "../../../../components/Checkbox";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import styles from "./CaptureDialog.module.css";

interface Props {
  onCancel: () => void;
  onCaptured: () => void;
}

/** Captures an existing project as a new blueprint. There is no `blueprint.delete` — overwriting is
 *  the only way to fix a slug that was named wrongly. */
export default function CaptureDialog({ onCancel, onCaptured }: Props) {
  const { t } = useTranslation();
  const [projectNames, setProjectNames] = useState<string[]>([]);
  const [project, setProject] = useState("");
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [overwrite, setOverwrite] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    api
      .projects()
      .then((list) => {
        setProjectNames(list.projects.map((p) => p.name));
        if (list.projects.length > 0) setProject(list.projects[0].name);
      })
      .catch((e: unknown) => setError(errorMessage(t, e)));
  }, [t]);

  async function submit() {
    setSaving(true);
    setError("");
    try {
      await api.blueprintCapture({
        project: { name: project },
        name,
        description: description.trim() === "" ? undefined : description,
        overwrite,
      });
      onCaptured();
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setSaving(false);
    }
  }

  return (
    <Modal
      title={t("mixengine.blueprints.capture.title")}
      onClose={onCancel}
      locked={saving}
      size="small"
      actions={[
        { kind: "cancel", label: t("common.cancel"), disabled: saving },
        {
          kind: "confirm",
          label: t("common.save"),
          onClick: () => void submit(),
          disabled: project === "" || name.trim() === "",
          busy: saving ? t("mixengine.blueprints.capture.saving") : undefined,
        },
      ]}
    >
      {() => (
        <>
          <ModalBody>
            <label className={styles.field}>
              {t("mixengine.blueprints.capture.project")}
              <Select
                value={project}
                onChange={setProject}
                disabled={saving || projectNames.length === 0}
                options={projectNames.map((n) => ({ value: n, label: n }))}
              />
            </label>

            <label className={styles.field}>
              {t("mixengine.blueprints.capture.name")}
              <Input value={name} disabled={saving} onChange={(e) => setName(e.target.value)} />
            </label>

            <label className={styles.field}>
              {t("mixengine.blueprints.capture.description")}
              <Input
                value={description}
                disabled={saving}
                onChange={(e) => setDescription(e.target.value)}
              />
            </label>

            <Checkbox
              className={styles.checkbox}
              label={t("mixengine.blueprints.capture.overwrite")}
              checked={overwrite}
              disabled={saving}
              onChange={(e) => setOverwrite(e.target.checked)}
            />
            {overwrite && (
              <p className={styles.hint}>{t("mixengine.blueprints.capture.overwriteHint")}</p>
            )}
          </ModalBody>
          <ModalErrors messages={[error]} />
        </>
      )}
    </Modal>
  );
}
