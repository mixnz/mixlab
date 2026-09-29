import { useCallback, useEffect, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";

import Button from "../../../../components/Button";
import ErrorBanner from "../../../../components/ErrorBanner";
import Input from "../../../../components/Input";
import Select from "../../../../components/Select";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { BlueprintApplied } from "@mixengine/api";
import type { BlueprintSummary } from "@mixengine/api";
import AfterApply from "../../components/AfterApply";
import ApplyDialog from "../Blueprints/ApplyDialog";
import { canStart } from "../../quickStart";
import styles from "./QuickStart.module.css";

/** How far the build has got. */
type Phase =
  | { kind: "form" }
  | { kind: "applying"; blueprint: BlueprintSummary }
  | { kind: "settling"; applied: BlueprintApplied }
  | { kind: "done"; url: string | null };

/**
 * Builds the first site in one click — T117.
 *
 * **A card, not a wizard.** A modal over a daemon that is installing a runtime is a modal in the
 * way; a screen of its own would have to justify itself forever, even once people have twenty
 * sites. This card is only drawn when `site.list` is empty, and it goes away by **getting the job
 * done**.
 *
 * **What follows the apply — allow → start → open — lives in [`AfterApply`]**, no longer here. It
 * used to live only here, so an apply from the Blueprints screen ended at a list of steps and a
 * Close button: services not started, domain not resolved, and no way to the site just built. This
 * card now only asks three questions, opens `ApplyDialog`, then gets an address back.
 */
export default function QuickStart({ onCreated }: { onCreated: () => void }) {
  const [available, setAvailable] = useState<BlueprintSummary[]>([]);
  const [slug, setSlug] = useState("");
  const [project, setProject] = useState("");
  const [root, setRoot] = useState("");
  const [phase, setPhase] = useState<Phase>({ kind: "form" });
  const [error, setError] = useState("");
  const { t } = useTranslation();

  const reload = useCallback(async () => {
    try {
      const listed = await api.blueprints();
      setAvailable(listed.blueprints);
      setSlug((current) => (current === "" ? (listed.blueprints[0]?.slug ?? "") : current));
      setError("");
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }, [t]);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function browse() {
    const picked = await openDialog({ directory: true, multiple: false });
    if (typeof picked === "string") setRoot(picked);
  }

  const chosen = available.find((blueprint) => blueprint.slug === slug);

  return (
    <section className={styles.card}>
      {error !== "" && <ErrorBanner message={error} onDismiss={() => setError("")} />}

      <h3 className={styles.title}>{t("mixengine.quickStart.title")}</h3>
      <p className={styles.intro}>{t("mixengine.quickStart.intro")}</p>

      {phase.kind === "done" ? (
        <div className={styles.doneRow}>
          <span>{t("mixengine.quickStart.done")}</span>
          {phase.url !== null && (
            <Button variant="primary" onClick={() => void openUrl(phase.url ?? "")}>
              {t("mixengine.quickStart.open", { url: phase.url })}
            </Button>
          )}
        </div>
      ) : (
        <div className={styles.form}>
          <label className={styles.field}>
            {t("mixengine.quickStart.stack")}
            <Select
              value={slug}
              onChange={setSlug}
              disabled={phase.kind !== "form"}
              searchable
              searchPlaceholder={t("mixengine.quickStart.searchStacks")}
              options={available.map((blueprint) => ({
                value: blueprint.slug,
                label: blueprint.name,
              }))}
            />
          </label>

          <label className={styles.field}>
            {t("mixengine.quickStart.name")}
            <Input
              value={project}
              disabled={phase.kind !== "form"}
              onChange={(e) => setProject(e.target.value)}
            />
          </label>

          <label className={styles.field}>
            {t("mixengine.quickStart.folder")}
            <div className={styles.folderRow}>
              <Input value={root} disabled={phase.kind !== "form"} readOnly />
              <Button onClick={() => void browse()} disabled={phase.kind !== "form"}>
                {t("mixengine.quickStart.browse")}
              </Button>
            </div>
          </label>

          <Button
            variant="primary"
            disabled={
              phase.kind !== "form" || chosen === undefined || !canStart(project, root)
            }
            onClick={() => chosen && setPhase({ kind: "applying", blueprint: chosen })}
          >
            {phase.kind === "settling"
              ? t("mixengine.quickStart.starting")
              : t("mixengine.quickStart.create")}
          </Button>
        </div>
      )}

      {phase.kind === "applying" && (
        <ApplyDialog
          blueprint={phase.blueprint}
          initialProject={project}
          initialRoot={root}
          withFrontEnd
          onCancel={() => setPhase({ kind: "form" })}
          onDone={(applied) => {
            if (applied === null) {
              setPhase({ kind: "form" });
              return;
            }
            setPhase({ kind: "settling", applied });
          }}
        />
      )}

      {/**
       * **`onCreated` is only called here, once everything is done — and that is a constraint, not
       * a preference.** The Dashboard draws this card if and only if `site.list` is empty
       * (`shouldOfferQuickStart`), so `onCreated` is the reread that makes this card **go away**.
       * Calling it earlier — right when the apply finishes, for instance — would unmount
       * `AfterApply` in the middle of its chain of three calls: the Dashboard's `site.list` comes
       * back within milliseconds, while that chain needs `elevation.status` + `service.start` +
       * `site.list`, so it loses for certain rather than only sometimes. The symptom is a project
       * that got built but whose browser never opens.
       */}
      {phase.kind === "settling" && (
        <AfterApply
          applied={phase.applied}
          onFinished={(url) => {
            setPhase({ kind: "done", url });
            onCreated();
          }}
        />
      )}
    </section>
  );
}
