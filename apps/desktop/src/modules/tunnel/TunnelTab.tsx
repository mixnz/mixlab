import { useCallback, useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";

import Button from "../../components/Button";
import Card from "../../components/Card";
import EmptyState from "../../components/EmptyState";
import ErrorBanner from "../../components/ErrorBanner";
import Input from "../../components/Input";
import PageHeader from "../../components/PageHeader";
import StatusPill from "../../components/StatusPill";
import Table from "../../components/Table";
import { copyText } from "../../core/clipboard";
import { errorMessage } from "../../core/errors";
import { useTranslation } from "../../i18n";
import type { ModuleTabProps } from "../../shell/module";
import * as api from "./api";
import { readCloudflaredPath } from "./settings";
import { formatSize, rowTone, type TunnelInfo } from "./tunnelState";
import styles from "./TunnelTab.module.css";

/**
 * The Tunnel tab — T203a. One list of every tunnel, read from the backend, which holds them so the
 * tray reads the same list and closing this tab stops nothing (D3).
 *
 * **The tab's session slot stays empty**: it is `localStorage`, which holds ids and never a host or
 * a URL, and a target is both. `onStateChange` is never called.
 */
export default function TunnelTab(_props: ModuleTabProps) {
  const { t } = useTranslation();
  const [binary, setBinary] = useState<api.BinaryStatus | null>(null);
  const [tunnels, setTunnels] = useState<TunnelInfo[]>([]);
  const [address, setAddress] = useState("");
  const [starting, setStarting] = useState(false);
  const [download, setDownload] = useState<{ done: number; total: number } | null>(null);
  const [copied, setCopied] = useState<number | null>(null);
  const [error, setError] = useState("");

  const readBinary = useCallback(() => {
    api.tunnelBinary(readCloudflaredPath()).then(setBinary, (e: unknown) => setError(errorMessage(t, e)));
  }, [t]);

  const readList = useCallback(() => {
    api.tunnelList().then(setTunnels, (e: unknown) => setError(errorMessage(t, e)));
  }, [t]);

  useEffect(() => {
    readBinary();
    readList();
    const changed = api.onChanged(readList);
    const progress = api.onDownload(setDownload);
    return () => {
      void changed.then((unlisten) => unlisten());
      void progress.then((unlisten) => unlisten());
    };
  }, [readBinary, readList]);

  async function fetchCloudflared() {
    setError("");
    setDownload({ done: 0, total: binary?.download?.size ?? 0 });
    try {
      await api.tunnelDownload();
      readBinary();
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setDownload(null);
    }
  }

  async function start() {
    setError("");
    setStarting(true);
    try {
      await api.tunnelStart(address, readCloudflaredPath());
      setAddress("");
      readList();
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setStarting(false);
    }
  }

  async function copy(info: TunnelInfo) {
    if (!info.url) return;
    try {
      await copyText(info.url);
      setCopied(info.id);
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }

  function stateLabel(info: TunnelInfo): string {
    if (info.state === "open") return t("tunnelTab.stateOpen");
    if (info.state === "failed") return t("tunnelTab.stateFailed");
    return t("tunnelTab.stateConnecting");
  }

  function hintText(info: TunnelInfo): string | null {
    if (info.hint === "allowedHosts") return t("tunnelTab.hintAllowedHosts");
    if (info.hint === "nothingListening") return t("tunnelTab.hintNothingListening", { target: info.target });
    return null;
  }

  const ready = binary?.found != null;

  return (
    <div className={styles.page}>
      {error !== "" && <ErrorBanner message={error} onDismiss={() => setError("")} />}

      <PageHeader title={t("tunnelTab.title")} description={t("tunnelTab.description")} />

      {binary !== null && !ready && (
        <Card title={t("tunnelTab.needCloudflared")}>
          <div className={styles.download}>
            {binary.download && (
              <span className={styles.note}>
                {t("tunnelTab.downloadSize", {
                  version: binary.download.version,
                  size: formatSize(binary.download.size),
                })}
              </span>
            )}
            {download !== null && (
              <progress
                className={styles.progress}
                value={download.total > 0 ? download.done : undefined}
                max={download.total > 0 ? download.total : undefined}
              />
            )}
            <Button
              variant="primary"
              disabled={binary.download === null}
              busy={download !== null ? t("tunnelTab.downloading") : undefined}
              onClick={() => void fetchCloudflared()}
            >
              {t("tunnelTab.download")}
            </Button>
          </div>
        </Card>
      )}

      <Card>
        <form
          className={styles.form}
          onSubmit={(event) => {
            event.preventDefault();
            void start();
          }}
        >
          <Input
            aria-label={t("tunnelTab.address")}
            placeholder={t("tunnelTab.addressPlaceholder")}
            value={address}
            mono
            onChange={(event) => setAddress(event.target.value)}
          />
          <Button
            type="submit"
            variant="primary"
            disabled={!ready || address.trim() === ""}
            busy={starting ? t("tunnelTab.stateConnecting") : undefined}
          >
            {t("tunnelTab.start")}
          </Button>
        </form>
        <p className={styles.note}>{t("tunnelTab.anyoneWithTheLink")}</p>
        <p className={styles.note}>{t("tunnelTab.forTesting")}</p>
        {binary?.found && (
          <p className={styles.note} title={binary.found.path}>
            {t("tunnelTab.using", { path: binary.found.path })}
          </p>
        )}
      </Card>

      <Card title={t("tunnelTab.running")} count={tunnels.length} flush>
        {tunnels.length === 0 ? (
          <EmptyState title={t("tunnelTab.empty")} />
        ) : (
          <Table aria-label={t("tunnelTab.running")}>
            <thead>
              <tr>
                <th>{t("tunnelTab.columnAddress")}</th>
                <th>{t("tunnelTab.columnState")}</th>
                <th>{t("tunnelTab.columnUrl")}</th>
                <th data-align="end" />
              </tr>
            </thead>
            <tbody>
              {tunnels.map((info) => {
                const hint = hintText(info);
                return (
                  <tr key={info.id}>
                    <td className={styles.mono}>{info.target}</td>
                    <td>
                      <StatusPill tone={rowTone(info)} pulse={info.state === "connecting"}>
                        {stateLabel(info)}
                      </StatusPill>
                    </td>
                    <td>
                      <span className={styles.url}>
                        {info.url && <span className={styles.mono}>{info.url}</span>}
                        {info.state === "failed" && info.detail && (
                          <span className={styles.detail} title={info.detail}>
                            {info.detail}
                          </span>
                        )}
                        {hint && <span className={styles.hint}>{hint}</span>}
                      </span>
                    </td>
                    <td data-align="end" data-nowrap>
                      <span className={styles.actions}>
                        {info.url && (
                          <>
                            <Button size="small" variant="soft" onClick={() => void copy(info)}>
                              {copied === info.id ? t("tunnelTab.copied") : t("tunnelTab.copy")}
                            </Button>
                            <Button size="small" variant="soft" onClick={() => void openUrl(info.url ?? "")}>
                              {t("tunnelTab.open")}
                            </Button>
                          </>
                        )}
                        <Button size="small" onClick={() => void api.tunnelStop(info.id)}>
                          {t("tunnelTab.stop")}
                        </Button>
                      </span>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </Table>
        )}
      </Card>
    </div>
  );
}
