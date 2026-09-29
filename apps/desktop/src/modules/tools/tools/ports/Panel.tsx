import { useCallback, useEffect, useMemo, useState } from "react";
import Button from "../../../../components/Button";
import Input from "../../../../components/Input";
import Select, { type SelectOption } from "../../../../components/Select";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import CopyField from "../../components/CopyField";
import { listeningPorts, type ListeningPort } from "./api";
import { matchesFilter } from "./filter";
import { hostOs, killByPid, killByPort, type KillOs } from "./kill";
import styles from "./Panel.module.css";

/** The labels are operating system names, so they are not translated. */
const OSES: SelectOption<KillOs>[] = [
  { value: "macos", label: "macOS" },
  { value: "linux", label: "Linux" },
  { value: "windows", label: "Windows" },
];

function PortsPanel() {
  const { t } = useTranslation();
  const [ports, setPorts] = useState<ListeningPort[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [selected, setSelected] = useState<ListeningPort | null>(null);
  /* The port number for the kill command block, separate from the table. Clicking a row fills it
     in, but it can also be typed by hand — the real need is killing a port on **another machine**,
     one this table cannot see. */
  const [killPort, setKillPort] = useState("");
  // Defaults to the machine it is running on, but can be changed by hand: people on Windows often
  // need a Linux command.
  const [os, setOs] = useState<KillOs>(hostOs);
  const [busy, setBusy] = useState(false);

  const load = useCallback(() => {
    setBusy(true);
    setError(null);
    void listeningPorts()
      .then((rows) => {
        setPorts(rows);
        // The selected port may have closed between two loads.
        setSelected((current) =>
          current && rows.some((row) => row.pid === current.pid && row.port === current.port)
            ? current
            : null,
        );
      })
      .catch((e: unknown) => {
        setError(errorMessage(t, e));
        setPorts([]);
      })
      .finally(() => setBusy(false));
    // `t` is not in the deps: it changes when the user switches language, and a rescan for that
    // reason is a wasted scan.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(load, [load]);

  const shown = useMemo(
    () => (ports ?? []).filter((row) => matchesFilter(row, filter)),
    [ports, filter],
  );

  return (
    <div className={styles.panel}>
      <div className={styles.controls}>
        <Button variant="primary" onClick={load} disabled={busy}>
          {busy ? t("common.loading") : t("toolbox.ports.refresh")}
        </Button>
        <Input
          value={filter}
          onChange={(event) => setFilter(event.target.value)}
          placeholder={t("toolbox.ports.filter")}
          aria-label={t("toolbox.ports.filter")}
          className={styles.filter}
        />
      </div>

      {error ? <p className={styles.error}>{error}</p> : null}

      {ports !== null && ports.length === 0 && !error ? (
        <p className={styles.note}>{t("toolbox.ports.empty")}</p>
      ) : null}

      {shown.length === 0 && (ports?.length ?? 0) > 0 ? (
        <p className={styles.note}>{t("toolbox.ports.noMatch")}</p>
      ) : null}

      {shown.length > 0 ? (
        <div className={styles.table}>
          <div className={`${styles.row} ${styles.head}`}>
            <span>{t("toolbox.ports.colPort")}</span>
            <span>{t("toolbox.ports.colAddress")}</span>
            <span>{t("toolbox.ports.colPid")}</span>
            <span>{t("toolbox.ports.colProcess")}</span>
          </div>
          {shown.map((row) => {
            const isSelected = selected?.pid === row.pid && selected?.port === row.port;
            return (
              <button
                key={`${row.address}:${row.port}:${row.pid}`}
                type="button"
                className={isSelected ? `${styles.row} ${styles.selected}` : styles.row}
                aria-current={isSelected ? "true" : undefined}
                onClick={() => {
                  setSelected(row);
                  setKillPort(String(row.port));
                }}
              >
                <span title={String(row.port)}>{row.port}</span>
                <span title={row.address}>{row.address}</span>
                <span title={String(row.pid)}>{row.pid}</span>
                <span
                  className={row.process ? undefined : styles.dim}
                  title={row.process ?? undefined}
                >
                  {row.process ?? t("toolbox.ports.unknownProcess")}
                </span>
              </button>
            );
          })}
        </div>
      ) : null}

      {/* This block **does not depend on the table above**: the real, common need is killing a
          port on another machine, possibly on another operating system, which this machine's table
          cannot see. Pick the OS, type the port number, copy the command. */}
      <section className={styles.kill}>
        <h3 className={styles.killTitle}>{t("toolbox.ports.killTitle")}</h3>
        <div className={styles.controls}>
          <Select
            value={os}
            options={OSES}
            onChange={setOs}
            ariaLabel={t("toolbox.ports.os")}
            className={styles.os}
          />
          <Input
            value={killPort}
            onChange={(event) => setKillPort(event.target.value.replace(/[^\d]/g, ""))}
            placeholder={t("toolbox.ports.portInput")}
            aria-label={t("toolbox.ports.portInput")}
            className={styles.port}
            inputMode="numeric"
          />
        </div>

        {killPort !== "" ? (
          <CopyField
            label={`${t("toolbox.ports.byPort")} · ${killPort}`}
            value={killByPort(os, Number(killPort))}
          />
        ) : null}

        {/* The PID-based command only means something for a row of **this machine**: the table
            does not know another machine's PIDs. */}
        {selected ? (
          <CopyField
            label={`${t("toolbox.ports.byPid")} · ${selected.pid}`}
            value={killByPid(os, selected.pid)}
          />
        ) : null}

        <p className={styles.note}>{t("toolbox.ports.note")}</p>
      </section>
    </div>
  );
}

export default PortsPanel;
