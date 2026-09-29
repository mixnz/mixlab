import Button from "../../../../components/Button";
import Card from "../../../../components/Card";
import Table from "../../../../components/Table";
import { useTranslation } from "../../../../i18n";
import type { OnDiskRow } from "../../onDisk";
import styles from "./OnDiskCard.module.css";

/**
 * Versions on disk that this home has not recorded, with **Adopt** — roadmap task T182i.
 *
 * Drawn only while the daemon lists some, at the top of Languages and of each Packages tab. An
 * *Install* whose directory was already there ends as a failed job, the tab reloads, and the version
 * appears here: the offer comes from the listing, never from reading the error.
 */
export default function OnDiskCard({
  rows,
  adopting,
  onAdopt,
}: {
  rows: OnDiskRow[];
  /** The key being adopted now, which locks the other buttons. */
  adopting: string | null;
  onAdopt: (row: OnDiskRow) => void;
}) {
  const { t } = useTranslation();

  if (rows.length === 0) return null;

  return (
    <Card
      title={t("mixengine.runtimes.onDisk.title")}
      description={t("mixengine.runtimes.onDisk.description")}
      count={rows.length}
      flush
    >
      <Table aria-label={t("mixengine.runtimes.onDisk.title")}>
        <tbody>
          {rows.map((row) => (
            <tr key={row.key}>
              <td>
                <div className={styles.name}>
                  {row.name} {row.version}
                </div>
                <div className={styles.detail}>{row.path}</div>
              </td>
              <td className={styles.detail}>{row.why}</td>
              <td data-align="end">
                <Button
                  variant="soft"
                  busy={adopting === row.key ? t("mixengine.runtimes.onDisk.adopting") : undefined}
                  disabled={adopting !== null && adopting !== row.key}
                  onClick={() => onAdopt(row)}
                >
                  {t("mixengine.runtimes.onDisk.adopt")}
                </Button>
              </td>
            </tr>
          ))}
        </tbody>
      </Table>
    </Card>
  );
}
