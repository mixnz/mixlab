import { useCallback, useEffect, useState } from "react";

import Button from "../../../../components/Button";
import Card from "../../../../components/Card";
import ErrorBanner from "../../../../components/ErrorBanner";
import LoadingState from "../../../../components/LoadingState";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import { rowsFrom, type ServiceRow } from "../../daemonState";
import { canChangePort, restartsOnMove } from "../../portDialogState";
import PortDialog from "./PortDialog";
import styles from "./PortPanel.module.css";

/**
 * The port a service listens on, and the way to move it.
 *
 * Drawn for every service with a port. Whether *Change…* is there is the daemon's answer
 * (`port_movable`): a front end or a php-fpm pool shows its port and one line saying it is fixed.
 * A service with no port draws nothing.
 */
export default function PortPanel({ service, dbVisible }: { service: string; dbVisible: boolean }) {
  const { t } = useTranslation();
  const [row, setRow] = useState<ServiceRow | null>(null);
  const [read, setRead] = useState(false);
  const [error, setError] = useState("");
  const [open, setOpen] = useState(false);

  const reload = useCallback(async () => {
    try {
      const list = await api.services();
      setRow(rowsFrom(list.services).find((one) => one.id === service) ?? null);
      setError("");
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setRead(true);
    }
  }, [service, t]);

  useEffect(() => {
    void reload();
  }, [reload]);

  if (read && (row === null || row.port === null) && error === "") return null;

  return (
    <Card
      headingLevel={3}
      title={t("mixengine.port.panelTitle")}
      description={t("mixengine.port.panelAbout")}
    >
      {error !== "" && <ErrorBanner message={error} onDismiss={() => setError("")} />}

      {!read ? (
        <LoadingState compact />
      ) : (
        row !== null &&
        row.port !== null && (
          <div className={styles.line}>
            <span className={styles.port}>{t("mixengine.port.current", { port: String(row.port) })}</span>
            {canChangePort(row) ? (
              <Button onClick={() => setOpen(true)}>{t("mixengine.port.changeButton")}</Button>
            ) : (
              <span className={styles.fixed}>{t("mixengine.port.notMovable")}</span>
            )}
          </div>
        )
      )}

      {open && row !== null && row.port !== null && (
        <PortDialog
          service={service}
          port={row.port}
          running={restartsOnMove(row.state)}
          onClose={() => setOpen(false)}
          onMoved={() => void reload()}
          offerSavedConnections={dbVisible}
        />
      )}
    </Card>
  );
}
