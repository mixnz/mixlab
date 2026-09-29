import { useState } from "react";

import Checkbox from "../../../../components/Checkbox";
import ConfirmDialog from "../../../../components/ConfirmDialog";
import { useTranslation } from "../../../../i18n";
import type { UpgradePlan } from "@mixengine/api";
import { formatBytes } from "../../metricsState";
import { oldVersionLine, planLines } from "../../upgradePlanLines";
import styles from "./UpgradeDialog.module.css";

interface Props {
  plan: UpgradePlan;
  onConfirm: (keep: boolean) => void;
  onCancel: () => void;
}

/**
 * The question an Update asks — T193, D7 and the design's MixLab section. Shows the daemon's plan
 * as it is, and one choice: keep the old version. When the plan already says it will be kept, the
 * box is ticked and cannot be cleared, and the reason is the line under it.
 */
export default function UpgradeDialog({ plan, onConfirm, onCancel }: Props) {
  const { t } = useTranslation();
  const forced = plan.old.state === "will_be_kept";
  const [keep, setKeep] = useState(forced);

  const message = plan.to_installed
    ? t("mixengine.upgrade.alreadyInstalled", { to: plan.to })
    : t("mixengine.upgrade.download", { size: formatBytes(plan.bytes) });

  return (
    <ConfirmDialog
      title={t("mixengine.upgrade.title", { name: plan.subject, from: plan.from, to: plan.to })}
      message={message}
      confirmLabel={t("mixengine.upgrade.confirm")}
      onCancel={onCancel}
      onConfirm={() => onConfirm(keep)}
    >
      <ul className={styles.lines}>
        {planLines(plan, t).map((line) => (
          <li key={line}>{line}</li>
        ))}
      </ul>
      <Checkbox
        label={t("mixengine.upgrade.keep", { from: plan.from })}
        checked={keep}
        disabled={forced}
        onChange={(event) => setKeep(event.target.checked)}
      />
      <p className={styles.old}>{oldVersionLine(plan, t)}</p>
    </ConfirmDialog>
  );
}
