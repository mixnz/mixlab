import SegmentedControl from "../../../../components/SegmentedControl";
import { useTranslation } from "../../../../i18n";
import { setCpuScale, useCpuScale } from "../../cpuScale";
import type { CpuScale } from "../../metricsState";
import styles from "./MixEngineSettings.module.css";

/**
 * The MixEngine module's pane in the app's Settings dialog: how its screens draw what the daemon
 * measures. Only this window's display — nothing here reaches the daemon, so the pane works with
 * MixEngine stopped or never started.
 */
function MixEngineSettings() {
  const { t } = useTranslation();
  const scale = useCpuScale();

  return (
    <div className={styles.group}>
      <span className={styles.groupLabel}>{t("mixengine.settingsCpuScale")}</span>
      <SegmentedControl<CpuScale>
        aria-label={t("mixengine.settingsCpuScale")}
        block
        value={scale}
        onChange={setCpuScale}
        segments={[
          { value: "machine", label: t("mixengine.settingsCpuMachine") },
          { value: "core", label: t("mixengine.settingsCpuCore") },
        ]}
      />
      <p className={styles.hint}>
        {t(scale === "core" ? "mixengine.settingsCpuCoreHint" : "mixengine.settingsCpuMachineHint")}
      </p>
    </div>
  );
}

export default MixEngineSettings;
