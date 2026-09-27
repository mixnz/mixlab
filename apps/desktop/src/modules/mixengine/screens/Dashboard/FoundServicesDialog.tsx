import Button from "../../../../components/Button";
import Modal, { ModalBody, ModalErrors } from "../../../../components/Modal";
import NoticeBanner from "../../../../components/NoticeBanner";
import Table from "../../../../components/Table";
import { useTranslation } from "../../../../i18n";
import type { FoundRow } from "../../foundServices";
import styles from "./FoundServices.module.css";

/**
 * The list behind `FoundServices`' *Review* — one row per data directory, with **Adopt** where the
 * daemon says an installed version opens it and the daemon's reason where it does not (T182g).
 */
export default function FoundServicesDialog({
  rows,
  adopting,
  adopted,
  error,
  onAdopt,
  onClose,
}: {
  rows: FoundRow[];
  /** The id being adopted now, which locks the dialog. */
  adopting: string | null;
  /** The id adopted last, said once above the list. */
  adopted: string | null;
  error: string;
  onAdopt: (id: string) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();

  return (
    <Modal
      title={t("mixengine.foundServices.dialogTitle")}
      onClose={onClose}
      locked={adopting !== null}
      actions={[{ kind: "cancel", label: t("common.close"), disabled: adopting !== null }]}
    >
      {() => (
        <>
          <ModalBody>
            <p className={styles.intro}>{t("mixengine.foundServices.dialogIntro")}</p>
            {adopted !== null && (
              <NoticeBanner message={t("mixengine.foundServices.adopted", { service: adopted })} />
            )}
            {rows.length === 0 ? (
              <p className={styles.intro}>{t("mixengine.foundServices.none")}</p>
            ) : (
              <Table aria-label={t("mixengine.foundServices.dialogTitle")}>
                <tbody>
                  {rows.map((row) => (
                    <tr key={row.id}>
                      <td>
                        <div className={styles.name}>{row.id}</div>
                        <div className={styles.path}>{row.path}</div>
                      </td>
                      <td className={styles.state}>
                        {row.adoptable
                          ? t("mixengine.foundServices.opensWith", { version: row.opensWith ?? "" })
                          : row.whyNot}
                      </td>
                      <td data-align="end">
                        {row.adoptable && (
                          <Button
                            variant="primary"
                            busy={adopting === row.id ? t("mixengine.foundServices.adopting") : undefined}
                            disabled={adopting !== null && adopting !== row.id}
                            onClick={() => onAdopt(row.id)}
                          >
                            {t("mixengine.foundServices.adopt")}
                          </Button>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </Table>
            )}
          </ModalBody>
          <ModalErrors messages={[error]} />
        </>
      )}
    </Modal>
  );
}
