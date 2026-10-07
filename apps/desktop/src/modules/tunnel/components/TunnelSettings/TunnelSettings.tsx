import { useState } from "react";

import Input from "../../../../components/Input";
import { useTranslation } from "../../../../i18n";
import { readCloudflaredPath, writeCloudflaredPath } from "../../settings";
import styles from "./TunnelSettings.module.css";

/** The Tunnel pane in Settings: which `cloudflared` to run — T203, D2. */
export default function TunnelSettings() {
  const { t } = useTranslation();
  const [path, setPath] = useState(readCloudflaredPath);

  return (
    <div className={styles.section}>
      <label className={styles.label} htmlFor="tunnel-cloudflared-path">
        {t("tunnelTab.pathLabel")}
      </label>
      <Input
        id="tunnel-cloudflared-path"
        value={path}
        mono
        allowClear
        onChange={(event) => {
          setPath(event.target.value);
          writeCloudflaredPath(event.target.value);
        }}
      />
      <p className={styles.hint}>{t("tunnelTab.pathHint")}</p>
    </div>
  );
}
