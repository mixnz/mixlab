import { useEffect, useState, type ReactNode } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import Button from "../../../../components/Button";
import Input from "../../../../components/Input";
import Modal, { ModalBody, ModalErrors } from "../../../../components/Modal";
import Select from "../../../../components/Select";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import { ChevronRightIcon } from "../../../../icons";
import * as api from "../../api";
import type { ProjectDetail } from "@mixengine/api";
import type { RuntimeKind } from "@mixengine/api";
import type { RuntimeSummary } from "@mixengine/api";
import { installedVersions } from "../../runtimeState";
import SiteFields, { draftDomains, emptySiteDraft, siteCreateFields } from "../../components/SiteFields";
import styles from "./ProjectForm.module.css";

// Every kind the contract knows, in the order the form shows them. `composer` arrived with
// MixEngine's T27c and was the first thing the alias onto `bindings/` caught (phase 11, T102): a
// project pinned to a Composer version would otherwise have lost that pin on its next save here.
// `go` arrived with T27d, before `composer`, in the order the contract's own list keeps; `java`
// arrived with T27e, after `go`.
const RUNTIME_KINDS: readonly RuntimeKind[] = ["php", "node", "python", "ruby", "go", "java", "composer"];

/**
 * A hand-built collapsible block — instead of a native `<details>`, for two reasons:
 *
 * **The icon and the motion can be chosen.** `<details>` only has the browser's default triangle,
 * whose style and speed cannot be changed. Here a `ChevronRightIcon` rotates 90° when open, in the
 * same mould as `Select`'s chevron (`Select.module.css`).
 *
 * **The height changes smoothly, without a jump.** The height goes from `0fr` to `1fr` on
 * `grid-template-rows` itself — the technique for animating to `auto` without measuring in JS.
 * `<details>` changes height instantly within one frame, and that is exactly what kept `Modal` from
 * re-centring itself on screen in time (see `dialogMotion.ts`, `onEntered`) before this existed.
 */
function Disclosure({
  summary,
  defaultOpen = false,
  demo,
  children,
}: {
  summary: ReactNode;
  defaultOpen?: boolean;
  /** `data-demo` on the summary button, for the promotional clips (`demo/clips.mjs`). */
  demo?: string;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div className={styles.disclosure}>
      <button
        type="button"
        className={styles.disclosureSummary}
        aria-expanded={open}
        onClick={() => setOpen((current) => !current)}
        data-demo={demo}
      >
        <ChevronRightIcon
          size="1.1em"
          className={`${styles.disclosureChevron} ${open ? styles.disclosureChevronOpen : ""}`}
        />
        {summary}
      </button>
      <div className={`${styles.disclosureBody} ${open ? styles.disclosureBodyOpen : ""}`}>
        <div className={styles.disclosureInner}>{children}</div>
      </div>
    </div>
  );
}

interface Props {
  /** `undefined` = create new. A value = edit. */
  initial?: ProjectDetail;
  onCancel: () => void;
  /** Called once saving is done — the parent does its own `reload()`. `warning` has a value when
   *  the project was created but the accompanying site (the "quick site" section) failed — the
   *  project still counts as saved, and the parent draws the warning itself. */
  onSaved: (warning?: string) => void;
}

export default function ProjectForm({ initial, onCancel, onSaved }: Props) {
  const { t } = useTranslation();
  const editing = initial !== undefined;

  const [root, setRoot] = useState(editing ? initial.project.root : "");
  const [name, setName] = useState(editing ? initial.project.name : "");
  const [pins, setPins] = useState<Record<RuntimeKind, string>>(() => {
    const initialPins: Record<RuntimeKind, string> = {
      php: "",
      node: "",
      python: "",
      ruby: "",
      go: "",
      java: "",
      composer: "",
    };
    if (editing) {
      for (const pin of initial.pins) initialPins[pin.kind] = pin.constraint;
    }
    return initialPins;
  });

  // "Quick site" — only shown when creating a new project (see the JSX). Leaving domains empty
  // skips this step entirely; it does not send an empty site to the daemon.
  // This site's root is always `root` (the field just above) — the same project, known from the
  // start, with no need to reread the way `SiteForm` must when the project is a separate choice.
  const [siteDraft, setSiteDraft] = useState(emptySiteDraft);
  const [serviceIds, setServiceIds] = useState<string[]>([]);

  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [installed, setInstalled] = useState<RuntimeSummary[]>([]);

  useEffect(() => {
    if (editing) return;
    void api.services().then((list) => setServiceIds(list.services.map((s) => s.id)));
  }, [editing]);

  /**
   * The installed versions, so each pin field can make suggestions instead of making people type
   * from memory.
   *
   * **Installed, not downloadable** — a constraint is only ever resolved against versions present
   * on this machine (`VersionConstraint`), so suggesting a version not installed invites the user
   * to pin something that will not resolve.
   *
   * On failure it is skipped: the pin field can still be typed by hand as before, and a
   * create-project dialog should not die because the suggestion list could not be read.
   */
  useEffect(() => {
    let live = true;
    void api
      .runtimesInstalled()
      .then((list) => live && setInstalled(list.runtimes))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, []);

  async function browseRoot() {
    const picked = await openDialog({ directory: true, multiple: false });
    if (typeof picked === "string") setRoot(picked);
  }

  function pinsPayload(): Partial<Record<RuntimeKind, string>> | undefined {
    const entries = RUNTIME_KINDS.filter((kind) => pins[kind].trim() !== "").map((kind) => [
      kind,
      pins[kind].trim(),
    ]);
    return entries.length > 0 ? Object.fromEntries(entries) : undefined;
  }

  const siteDomains = draftDomains(siteDraft);

  async function submit() {
    setSaving(true);
    setError("");
    try {
      if (editing) {
        await api.projectUpdate({
          project: { name: initial.project.name },
          name: name !== initial.project.name ? name : undefined,
          root: root !== initial.project.root ? root : undefined,
          // No `keep_warm`: absent leaves the column as it is. The form no longer shows it — it
          // only matters while Save battery is on, and `mix project keep-warm` sets it (T167g).
          pins: pinsPayload() ?? {},
        });
        onSaved();
        return;
      }

      const created = await api.projectCreate({
        root,
        name: name.trim() === "" ? undefined : name,
        pins: pinsPayload(),
      });
      if (siteDomains.length > 0) {
        try {
          await api.siteCreate({ project: { name: created.project.name }, ...siteCreateFields(siteDraft) });
        } catch (e) {
          // The project was saved — this is a warning about the site alone, not a failed save.
          // Closing the dialog is still right: reopening it just to retype the same project part
          // would hit a "name already exists" error straight away, since `project.create` really
          // did just run.
          onSaved(t("mixengine.projects.form.siteFailed", { error: errorMessage(t, e) }));
          return;
        }
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
      title={t(editing ? "mixengine.projects.form.editTitle" : "mixengine.projects.form.createTitle")}
      onClose={onCancel}
      locked={saving}
      actions={[
        { kind: "cancel", label: t("common.cancel"), disabled: saving },
        {
          kind: "confirm",
          label: t("common.save"),
          onClick: () => void submit(),
          disabled: root.trim() === "",
          busy: saving ? t("mixengine.projects.form.saving") : undefined,
          demo: "project-save",
        },
      ]}
    >
      {() => (
        <>
          <ModalBody>
            <label className={styles.field}>
              {t("mixengine.projects.form.root")}
              <div className={styles.rootRow}>
                <Input value={root} disabled={saving} onChange={(e) => setRoot(e.target.value)} />
                <Button onClick={() => void browseRoot()} disabled={saving}>
                  {t("common.browse")}
                </Button>
              </div>
              {editing && <p className={styles.hint}>{t("mixengine.projects.form.rootMovedHint")}</p>}
            </label>

            <label className={styles.field}>
              {t("mixengine.projects.form.name")}
              <Input
                value={name}
                disabled={saving}
                onChange={(e) => setName(e.target.value)}
                placeholder={t("mixengine.projects.form.namePlaceholder")}
              />
            </label>

            <Disclosure summary={t("mixengine.projects.form.pinsSummary")} demo="project-pins">
              {/* `freeText` rather than a plain Select: the value here is a `VersionConstraint`,
                  and the list is only the installed versions — `^8.3` or `8.3` is not in it but
                  still has to be pinnable. */}
              {RUNTIME_KINDS.map((kind) => (
                <label key={kind} className={styles.field} data-demo-key={kind}>
                  {kind}
                  <Select
                    demo="project-pin"
                    value={pins[kind]}
                    freeText
                    disabled={saving}
                    placeholder={t("mixengine.projects.form.pinAny")}
                    searchPlaceholder={t("mixengine.projects.form.pinPlaceholder")}
                    onChange={(value) => setPins((prev) => ({ ...prev, [kind]: value }))}
                    options={[
                      { value: "", label: t("mixengine.projects.form.pinAny") },
                      ...installedVersions(installed, kind).map((version) => ({
                        value: version,
                        label: version,
                      })),
                    ]}
                  />
                </label>
              ))}
            </Disclosure>

            {/* Only in the create form — editing an existing project is not the moment to bundle in
                a site; its site (if any) can already be edited separately on the Sites screen. */}
            {!editing && (
              <Disclosure summary={t("mixengine.projects.form.quickSiteSummary")}>
                <SiteFields
                  value={siteDraft}
                  onChange={setSiteDraft}
                  serviceIds={serviceIds}
                  projectRoot={root}
                  disabled={saving}
                />
              </Disclosure>
            )}
          </ModalBody>
          <ModalErrors messages={[error]} />
        </>
      )}
    </Modal>
  );
}
