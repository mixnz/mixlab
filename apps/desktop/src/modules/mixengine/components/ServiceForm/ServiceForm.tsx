import { useEffect, useState } from "react";

import Input from "../../../../components/Input";
import Modal, { ModalBody, ModalErrors } from "../../../../components/Modal";
import Select from "../../../../components/Select";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { PackageSummary } from "@mixengine/api";
import type { ServiceCreation } from "@mixengine/api";
import { versionKey } from "../../runtimeState";
import { DEFAULT_INSTANCE, serviceIdFrom, takesInstanceName } from "./serviceId";
import styles from "./ServiceForm.module.css";

interface Props {
  onCancel: () => void;
  /** Called once creation is done — the parent does its own `reload()` and tells the `moved_from`
   *  story itself. */
  onCreated: (created: ServiceCreation) => void;
}

/**
 * Builds a new service instance from a package already on disk.
 *
 * **Packages only, not runtimes.** `php-fpm@<version>` is born together with a PHP install and is
 * not something built by hand here; this list is `package.list`, exactly what `service.create`
 * takes.
 *
 * **One choice for both name and version.** `ServiceCreate` requires `version` and deliberately
 * has no `service.resolve` to pick one for you, but two separate `Select`s are two values that can
 * drift apart — so each row here is a real `package.list` row, not a pair the user put together.
 */
export default function ServiceForm({ onCancel, onCreated }: Props) {
  const { t } = useTranslation();

  /** `null` is not finished asking — quite different from `[]`, which means asked, and there are
   *  no packages on the machine. */
  const [packages, setPackages] = useState<PackageSummary[] | null>(null);
  const [picked, setPicked] = useState("");
  const [instance, setInstance] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    void api.packagesInstalled().then(
      (list) => setPackages(list.packages),
      (e: unknown) => {
        setPackages([]);
        setError(errorMessage(t, e));
      },
    );
  }, [t]);

  const chosen = (packages ?? []).find((row) => versionKey(row.package, row.version) === picked);
  const nothingInstalled = packages !== null && packages.length === 0;
  /* A front end has no field to type into, so an `instance` left over from a previous choice must
     not follow along into the id either. */
  const named = chosen !== undefined && takesInstanceName(chosen.package);
  const id = chosen === undefined ? "" : serviceIdFrom(chosen.package, named ? instance : "");
  const incomplete = chosen === undefined || (named && instance.trim() === "");

  /** Changing the package also changes the question "is there an instance name", so that field is
   *  reset according to the package. */
  function pick(next: string) {
    setPicked(next);
    const row = (packages ?? []).find((p) => versionKey(p.package, p.version) === next);
    setInstance(row !== undefined && takesInstanceName(row.package) ? DEFAULT_INSTANCE : "");
  }

  async function submit() {
    if (chosen === undefined) return;
    setSaving(true);
    setError("");
    try {
      onCreated(await api.serviceCreate({ id, version: chosen.version }));
    } catch (e) {
      // Stay in the modal with the choice just made: what the daemon refuses ("another web server
      // holds port 80") is usually fixed by changing one field, not by retyping the whole form.
      setError(errorMessage(t, e));
    } finally {
      setSaving(false);
    }
  }

  return (
    <Modal
      title={t("mixengine.serviceForm.title")}
      onClose={onCancel}
      locked={saving}
      size="small"
      actions={[
        { kind: "cancel", label: t("common.cancel"), disabled: saving },
        {
          kind: "confirm",
          label: t("common.save"),
          onClick: () => void submit(),
          disabled: incomplete,
          busy: saving ? t("mixengine.serviceForm.saving") : undefined,
        },
      ]}
    >
      {() => (
        <>
          <ModalBody>
            <label className={styles.field}>
              {t("mixengine.serviceForm.package")}
              {/* `searchable`: the list grows with the number of installed packages, and each
                  version is a row of its own — on a machine holding two MariaDB versions next to
                  Redis, Postgres and two web servers, scrolling takes longer than typing. `Select`
                  matches on the label, and the label here is "name version", so typing `maria` or
                  `11.4` both get there. */}
              {nothingInstalled ? (
                <p className={styles.hint}>{t("mixengine.serviceForm.noPackages")}</p>
              ) : (
                <Select
                  value={picked}
                  onChange={pick}
                  searchable
                  disabled={packages === null || saving}
                  placeholder={t("mixengine.serviceForm.packagePlaceholder")}
                  options={(packages ?? []).map((row) => ({
                    value: versionKey(row.package, row.version),
                    label: `${row.package} ${row.version}`,
                  }))}
                />
              )}
            </label>

            {named && (
              <label className={styles.field}>
                {t("mixengine.serviceForm.instance")}
                <Input
                  value={instance}
                  disabled={saving}
                  onChange={(e) => setInstance(e.target.value)}
                  placeholder={t("mixengine.serviceForm.instancePlaceholder")}
                />
              </label>
            )}

            {id !== "" && (
              <p className={styles.hint}>
                {t("mixengine.serviceForm.idPreview", { id })}
              </p>
            )}
          </ModalBody>
          <ModalErrors messages={[error]} />
        </>
      )}
    </Modal>
  );
}
