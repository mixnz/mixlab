import { useEffect, useState } from "react";

import Modal, { ModalBody, ModalErrors } from "../../../../components/Modal";
import Select from "../../../../components/Select";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { SiteDetail } from "@mixengine/api";
import SiteFields, {
  emptySiteDraft,
  siteCreateFields,
  siteDraftFromDetail,
  siteUpdateFields,
} from "../../components/SiteFields";
import styles from "./SiteForm.module.css";

interface Props {
  /** `undefined` = create new. A value = edit, with the project locked. */
  initial?: SiteDetail;
  /** Preselected when creating — Sites passes the project it is filtering by, if any. Ignored when
   *  `initial` has a value. */
  defaultProject?: string;
  onCancel: () => void;
  /** Called once saving is done — the parent does its own `reload()`. */
  onSaved: () => void;
}

export default function SiteForm({ initial, defaultProject, onCancel, onSaved }: Props) {
  const { t } = useTranslation();
  const editing = initial !== undefined;

  const [projectNames, setProjectNames] = useState<string[]>([]);
  const [serviceIds, setServiceIds] = useState<string[]>([]);
  // `false` until both lists above have arrived — see where it is used just before `return`.
  const [listsReady, setListsReady] = useState(false);

  const [project, setProject] = useState(
    editing && initial.site.owner.type === "project"
      ? initial.site.owner.name
      : (defaultProject ?? ""),
  );
  // The root of the project owning the site — for create, reread every time the project changes
  // (below); for edit, `SiteDetail.root` is already there and the project is locked, so it no
  // longer changes. Display only: the value sent to the daemon is always the remainder on its own
  // (`docRoot`), exactly `SiteSummary.doc_root`.
  const [projectRoot, setProjectRoot] = useState(editing ? initial.root : "");
  // Whether the first answer has arrived yet — different from `projectRoot !== ""`, because "" is
  // a valid answer (no project chosen, or a genuinely empty project). Only holds back the Modal the
  // first time; changing the project after the Modal has opened does not close it — see where it
  // is used just before `return`.
  const [projectRootReady, setProjectRootReady] = useState(editing);
  const [draft, setDraft] = useState(() => (editing ? siteDraftFromDetail(initial) : emptySiteDraft()));

  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    void Promise.all([api.projects(), api.services()]).then(([projectList, serviceList]) => {
      setProjectNames(projectList.projects.map((p) => p.name));
      setServiceIds(serviceList.services.map((s) => s.id));
      setListsReady(true);
    });
  }, []);

  // Only for create — for edit the project is locked and `initial.root` is already the right root,
  // which no longer changes.
  useEffect(() => {
    if (editing) return;
    if (project === "") {
      setProjectRoot("");
      setProjectRootReady(true);
      return;
    }
    let live = true;
    void api
      .projectShow(project)
      .then((detail) => {
        if (!live) return;
        setProjectRoot(detail.project.root);
        setProjectRootReady(true);
      })
      .catch(() => {
        if (!live) return;
        setProjectRoot("");
        setProjectRootReady(true);
      });
    return () => {
      live = false;
    };
  }, [editing, project]);

  const noProjects = !editing && projectNames.length === 0;

  // Not enough data yet to know the form's final shape — do not open the Modal yet. Without
  // waiting, the service list (empty at first, filled once `api.services()` answers) and the "Full
  // path" line (waiting for `projectRoot`) stretch the dialog's height while its opening animation
  // is still running — the dialog animates at one height, grows taller midway, and that is exactly
  // the reported modal "snapping upwards". `onEntered`/`.settled` (`dialogMotion.ts`) only cover
  // changes *after* the animation finishes; changes *while* it runs have to be avoided at the
  // source, not covered afterwards. The calls are local over IPC so they usually finish within a
  // frame — a very short pause before opening is better than a jolt after it has opened.
  if (!listsReady || !projectRootReady) return null;

  async function submit() {
    setSaving(true);
    setError("");
    try {
      if (editing) {
        await api.siteUpdate({ site: { domain: initial.site.domain }, ...siteUpdateFields(draft) });
      } else {
        await api.siteCreate({ project: { name: project }, ...siteCreateFields(draft) });
      }
      onSaved();
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setSaving(false);
    }
  }

  return (
    <Modal
      title={t(editing ? "mixengine.sites.form.editTitle" : "mixengine.sites.form.createTitle")}
      onClose={onCancel}
      locked={saving}
      actions={[
        { kind: "cancel", label: t("common.cancel"), disabled: saving },
        {
          kind: "confirm",
          label: t("common.save"),
          onClick: () => void submit(),
          disabled: !editing && (project === "" || noProjects),
          busy: saving ? t("mixengine.sites.form.saving") : undefined,
        },
      ]}
    >
      {() => (
        <>
          <ModalBody>
            {!editing && (
              <label className={styles.field}>
                {t("mixengine.sites.form.project")}
                {noProjects ? (
                  <p className={styles.hint}>{t("mixengine.sites.form.noProjects")}</p>
                ) : (
                  <Select
                    value={project}
                    onChange={setProject}
                    disabled={saving}
                    options={projectNames.map((name) => ({ value: name, label: name }))}
                    placeholder={t("mixengine.sites.form.project")}
                  />
                )}
              </label>
            )}

            <SiteFields
              value={draft}
              onChange={setDraft}
              serviceIds={serviceIds}
              projectRoot={projectRoot}
              disabled={saving}
              showEnabled={editing}
            />
          </ModalBody>
          <ModalErrors messages={[error]} />
        </>
      )}
    </Modal>
  );
}
