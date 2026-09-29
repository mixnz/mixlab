import { useState } from "react";

import Modal, { ModalBody, ModalErrors } from "../../../../components/Modal";
import Checkbox from "../../../../components/Checkbox";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { DiskUsage } from "@mixengine/api";
import { cleanupFlagFor, isCleanupReclaimable } from "../../diskUsageState";
import styles from "./CleanupDialog.module.css";

interface Props {
  disk: DiskUsage;
  onCancel: () => void;
  onStarted: () => void;
}

/**
 * Cleans up `logs`/`cache` through `daemon.cleanup`.
 *
 * **Only two checkboxes, always exactly two** — `Reclaim::ByCleanup` is only ever attached to
 * `logs`/`cache` (just as the `Cleaned` doc comment says), so the list is filtered from
 * `disk.categories` rather than hard-coded, so that a machine where the daemon says otherwise still
 * draws correctly instead of drawing a dead checkbox. Unticked means clean (the default) —
 * `CleanupQuery.keep_logs`/`keep_cache` default to `false`.
 */
export default function CleanupDialog({ disk, onCancel, onStarted }: Props) {
  const { t } = useTranslation();
  const reclaimable = disk.categories.filter(isCleanupReclaimable);
  const [keep, setKeep] = useState<Record<string, boolean>>({});
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState("");

  async function submit() {
    setSubmitting(true);
    setError("");
    try {
      await api.cleanup({
        keep_logs: keep.logs ?? false,
        keep_cache: keep.cache ?? false,
      });
      onStarted();
    } catch (e) {
      setError(errorMessage(t, e));
      setSubmitting(false);
    }
  }

  return (
    <Modal
      title={t("mixengine.dashboard.diskUsage.cleanup")}
      onClose={onCancel}
      locked={submitting}
      size="small"
      actions={[
        { kind: "cancel", label: t("common.cancel"), disabled: submitting },
        {
          kind: "confirm",
          label: t("mixengine.dashboard.diskUsage.cleanup"),
          onClick: () => void submit(),
          disabled: submitting,
        },
      ]}
    >
      {() => (
        <>
          <ModalBody>
            <div className={styles.list}>
              {reclaimable.map((category) => {
                const flag = cleanupFlagFor(category.id);
                if (flag === null || category.reclaim.reclaim !== "by_cleanup") return null;
                return (
                  <Checkbox
                    key={category.id}
                    className={styles.item}
                    label={t("mixengine.dashboard.diskUsage.cleanupKeep", {
                      category: t(`mixengine.dashboard.diskUsage.category.${category.id}`),
                    })}
                    checked={keep[category.id] ?? false}
                    disabled={submitting}
                    onChange={(e) =>
                      setKeep((current) => ({ ...current, [category.id]: e.target.checked }))
                    }
                  />
                );
              })}
            </div>
          </ModalBody>
          <ModalErrors messages={[error]} />
        </>
      )}
    </Modal>
  );
}
