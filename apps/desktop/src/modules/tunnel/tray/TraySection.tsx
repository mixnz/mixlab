import { useCallback, useEffect, useState } from "react";

import Button from "../../../components/Button";
import StatusPill from "../../../components/StatusPill";
import { copyText } from "../../../core/clipboard";
import { TunnelIcon } from "../../../icons";
import { useTranslation } from "../../../i18n";
import type { TraySectionProps } from "../../../shell/module";
import * as api from "../api";
import { rowTone, type TunnelInfo } from "../tunnelState";
import styles from "./TraySection.module.css";

/**
 * The Tunnel section of the tray panel — T203b, D5. What is published right now, from the menu bar,
 * so a person who closed the window can see it and end it there.
 *
 * **Always lent, and one line when nothing runs**: `TraySection` is part of the module's definition,
 * not of its state. The list is the backend's, the one the tab reads.
 */
function TraySection({ focused }: TraySectionProps) {
  const { t } = useTranslation();
  const [tunnels, setTunnels] = useState<TunnelInfo[]>([]);
  const [copied, setCopied] = useState<number | null>(null);

  const refresh = useCallback(() => {
    api.tunnelList().then(setTunnels, () => setTunnels([]));
  }, []);

  useEffect(() => {
    if (focused) refresh();
  }, [focused, refresh]);

  useEffect(() => {
    const changed = api.onChanged(refresh);
    return () => {
      void changed.then((unlisten) => unlisten());
    };
  }, [refresh]);

  async function copy(info: TunnelInfo) {
    if (!info.url) return;
    try {
      await copyText(info.url);
      setCopied(info.id);
    } catch {
      setCopied(null);
    }
  }

  return (
    <div className={styles.section}>
      <div className={styles.head}>
        <span className={styles.mark}>
          <TunnelIcon size={16} />
        </span>
        <span className={styles.name}>{t("tunnelTab.title")}</span>
      </div>
      {tunnels.length === 0 ? (
        <p className={styles.empty}>{t("tunnelTab.empty")}</p>
      ) : (
        <ul className={styles.list}>
          {tunnels.map((info) => (
            <li key={info.id} className={styles.row}>
              <span className={styles.text}>
                <span className={styles.target}>{info.target}</span>
                <span className={styles.url} title={info.url ?? undefined}>
                  {info.url ?? ""}
                </span>
                {info.hint && (
                  <span className={styles.hint}>
                    {info.hint === "allowedHosts"
                      ? t("tunnelTab.hintAllowedHosts")
                      : t("tunnelTab.hintNothingListening", { target: info.target })}
                  </span>
                )}
              </span>
              <StatusPill tone={rowTone(info)} pulse={info.state === "connecting"}>
                {info.state === "open"
                  ? t("tunnelTab.stateOpen")
                  : info.state === "failed"
                    ? t("tunnelTab.stateFailed")
                    : t("tunnelTab.stateConnecting")}
              </StatusPill>
              {info.url && (
                <Button size="small" variant="soft" onClick={() => void copy(info)}>
                  {copied === info.id ? t("tunnelTab.copied") : t("tunnelTab.copy")}
                </Button>
              )}
              <Button size="small" onClick={() => void api.tunnelStop(info.id)}>
                {t("tunnelTab.stop")}
              </Button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export default TraySection;
