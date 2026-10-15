import { useState } from "react";
import { TrashIcon } from "../../../../icons";
import { useTranslation } from "../../../../i18n";
import { removeSnippet, saveSnippet, useQuerySnippets } from "../../queries/querySnippets";
import Button from "../../../../components/Button";
import Input from "../../../../components/Input";
import styles from "./QueryEditor.module.css";
import Modal, { ModalBody, ModalErrors } from "../../../../components/Modal";

interface Props {
  /** The query the Save row would keep — the selection, or the statement the caret is in. Empty
   *  when the editor has nothing in it, and then the row explains itself rather than disappearing. */
  sql: string;
  /** Puts a saved query back in the editor. */
  onPick: (sql: string) => void;
  onClose: () => void;
}

/** The query on one line, as it appears in the list and in the completion detail. */
function oneLine(sql: string): string {
  return sql.replace(/\s+/g, " ").trim();
}

/**
 * The saved queries: what is kept, and the one place a new one is added or an old one dropped.
 *
 * Saving and forgetting live together on purpose. They are the same thought — "which queries am I
 * keeping?" — and splitting them would have meant a second button in a toolbar that already has
 * five, plus a screen nobody would find. Opening this to save one is also how you see what is
 * already there, which is what stops a name being used twice by accident.
 */
function QuerySnippetsDialog({ sql, onPick, onClose }: Props) {
  const { t } = useTranslation();
  const snippets = useQuerySnippets();
  const [name, setName] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  /** The snippet whose delete button has been pressed once. Two presses rather than a dialog on
   *  top of this one — and only ever one at a time, so the armed button is unmistakable. */
  const [confirmDrop, setConfirmDrop] = useState<string | null>(null);

  const canSave = sql.trim() !== "";

  async function save() {
    if (!canSave || name.trim() === "") return;
    setSaving(true);
    setError("");
    try {
      await saveSnippet({ name, sql });
      setName("");
    } catch {
      setError(t("query.snippetSaveFailed"));
    } finally {
      setSaving(false);
    }
  }

  async function drop(snippetName: string) {
    setConfirmDrop(null);
    try {
      await removeSnippet(snippetName);
    } catch {
      setError(t("query.snippetDeleteFailed"));
    }
  }

  return (
    <Modal
      title={t("query.snippetsTitle")}
      onClose={onClose}
      locked={saving}
      fixedHeight
    >
      {(close) => (
        <>
          <ModalBody>
            <div className={styles.historyTools}>
              <Input
                value={name}
                onChange={(e) => setName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void save();
                }}
                placeholder={t("query.snippetNamePlaceholder")}
                aria-label={t("query.snippetNamePlaceholder")}
                disabled={!canSave || saving}
                autoFocus
              />
              <Button
                size="small"
                variant="primary"
                onClick={() => void save()}
                disabled={!canSave || saving || name.trim() === ""}
              >
                {saving ? t("query.snippetSaving") : t("query.saveSnippet")}
              </Button>
            </div>
            <p className={styles.historyHint}>
              {canSave ? oneLine(sql).slice(0, 120) : t("query.snippetNothingToSave")}
            </p>

            {snippets.length === 0 ? (
              <p className={styles.historyEmpty}>{t("query.snippetsEmpty")}</p>
            ) : (
              <ul className={styles.historyList} onMouseDown={() => setConfirmDrop(null)}>
                {snippets.map((snippet) => (
                  <li key={snippet.name} className={styles.entryRow}>
                    <button
                      type="button"
                      className={styles.historyEntry}
                      title={snippet.sql}
                      onClick={() => {
                        onPick(snippet.sql);
                        close(onClose);
                      }}
                    >
                      <span className={styles.snippetName}>{snippet.name}</span>
                      <span className={styles.historySql}>{oneLine(snippet.sql)}</span>
                    </button>
                    <button
                      type="button"
                      className={
                        confirmDrop === snippet.name
                          ? `${styles.entryDelete} ${styles.entryDeleteArmed}`
                          : styles.entryDelete
                      }
                      title={
                        confirmDrop === snippet.name
                          ? t("query.snippetDeleteConfirm")
                          : t("query.snippetDelete", { name: snippet.name })
                      }
                      // The list disarms on mouse-down, which lands before this button's click and
                      // would clear `confirmDrop` in time for the confirming press to read it as
                      // unarmed — the second click re-arming for ever instead of deleting. Stopping it
                      // here keeps disarming to presses that are somewhere else.
                      onMouseDown={(e) => e.stopPropagation()}
                      onClick={() =>
                        confirmDrop === snippet.name ? void drop(snippet.name) : setConfirmDrop(snippet.name)
                      }
                    >
                      <TrashIcon size="0.9em" />
                      {confirmDrop === snippet.name && <span>{t("query.snippetDeleteConfirm")}</span>}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </ModalBody>
          <ModalErrors messages={[error]} />
        </>
      )}
    </Modal>
  );
}

export default QuerySnippetsDialog;
