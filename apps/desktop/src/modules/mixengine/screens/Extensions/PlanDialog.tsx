import { useEffect, useState } from "react";

import Modal, { ModalBody, ModalErrors } from "../../../../components/Modal";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { ExtensionOrigin } from "@mixengine/api";
import type { ExtensionPlan } from "@mixengine/api";
import styles from "./PlanDialog.module.css";

interface Props {
  source: ExtensionOrigin;
  onCancel: () => void;
  /** The daemon accepted the install job: which add-on, and the job to follow. */
  onStarted: (id: string, job: number) => void;
}

/**
 * Previews an install before sending it, for both registry and local directory sources.
 *
 * `extension.plan` is the only step — `extension.inspect` is not called first (Decision D2, spec).
 * Sign-in (`site.signs_in`) is drawn **inside** the permissions block, not next to the domain
 * (spec, section 2).
 *
 * Closes once the daemon has accepted the install job; the screen follows the job on its row
 * (T200, D3). A modal locked for a minute-long download would hold the whole window. The id comes
 * from the plan because a path install's id is unknown until its manifest has been read.
 */
export default function PlanDialog({ source, onCancel, onStarted }: Props) {
  const { t } = useTranslation();
  const [plan, setPlan] = useState<ExtensionPlan | null>(null);
  const [error, setError] = useState("");
  const [installing, setInstalling] = useState(false);

  useEffect(() => {
    api
      .extensionPlan({ source })
      .then(setPlan)
      .catch((e: unknown) => setError(errorMessage(t, e)));
  }, [source, t]);

  async function install() {
    if (!plan) return;
    setInstalling(true);
    setError("");
    try {
      // `consent` is taken verbatim from `plan` — not rebuilt from input; see Decision D3.
      const job = await api.extensionInstall({
        source,
        consent: {
          id: plan.id,
          version: plan.version,
          signed: plan.signed,
          network: plan.permissions.network,
        },
      });
      onStarted(plan.id, job.id);
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setInstalling(false);
    }
  }

  return (
    <Modal
      title={t("mixengine.extensions.plan.title", { name: plan?.name ?? "" })}
      onClose={onCancel}
      locked={installing}
      size="small"
      actions={[
        { kind: "cancel", label: t("common.cancel"), disabled: installing },
        {
          kind: "confirm",
          label: t("mixengine.extensions.plan.installButton"),
          onClick: () => void install(),
          disabled: !plan,
          busy: installing ? t("mixengine.extensions.plan.installing") : undefined,
        },
      ]}
    >
      {() => (
        <>
          <ModalBody>
            {plan && (
              <div className={styles.body}>
                {!plan.signed && (
                  <p className={styles.warning}>{t("mixengine.extensions.plan.unsigned")}</p>
                )}
                <p>{plan.description}</p>
                {plan.homepage && (
                  <p>{t("mixengine.extensions.plan.homepage", { url: plan.homepage })}</p>
                )}

                <h4>{t("mixengine.extensions.plan.permissionsTitle")}</h4>
                <ul className={styles.list}>
                  {plan.permissions.services.map((access, i) => (
                    <li key={i}>
                      {access === "read"
                        ? t("mixengine.extensions.plan.apiRead")
                        : t("mixengine.extensions.plan.apiWrite")}
                    </li>
                  ))}
                  <li>{t("mixengine.extensions.plan.network", { reach: plan.permissions.network })}</li>
                  {plan.permissions.filesystem.map((reach, i) => (
                    <li key={i}>{t("mixengine.extensions.plan.filesystem", { reach })}</li>
                  ))}
                  {plan.site?.signs_in && (
                    <li>{t("mixengine.extensions.plan.signsIn", { account: plan.site.signs_in })}</li>
                  )}
                </ul>

                {plan.site && (
                  <p>
                    {t("mixengine.extensions.plan.site", {
                      domain: plan.site.domain,
                      pool: plan.site.pool,
                    })}
                    {plan.site.database && (
                      <> — {t("mixengine.extensions.plan.database", { database: plan.site.database })}</>
                    )}
                  </p>
                )}

                {plan.ports.map((port, i) => (
                  <p key={i}>
                    {t("mixengine.extensions.plan.ports", { name: port.name, wanted: port.wanted })}
                  </p>
                ))}

                <p>{t("mixengine.extensions.plan.installDir", { dir: plan.install_dir })}</p>
                <p>{t("mixengine.extensions.plan.dataDir", { dir: plan.data_dir })}</p>
              </div>
            )}
          </ModalBody>
          <ModalErrors messages={[error]} />
        </>
      )}
    </Modal>
  );
}
