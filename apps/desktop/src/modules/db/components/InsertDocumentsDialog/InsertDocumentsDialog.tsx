import { useEffect, useRef, useState } from "react";
import Button from "../../../../components/Button";
import Document from "../Document";
import { PlusIcon } from "../../../../icons";
import { useTranslation } from "../../../../i18n";
import { errorMessage } from "../../../../core/errors";
import type { TypedDocument, TypedValue } from "../../mongo/bsonTypes";
import styles from "./InsertDocumentsDialog.module.css";
import Modal, { ModalBody, ModalErrors } from "../../../../components/Modal";

/** One document being composed. The data itself lives in the card — `seed` is only what the
 * card was mounted with, kept as the fallback for a draft that has yet to report anything. */
interface Draft {
  key: number;
  seed: TypedDocument;
}

interface Props {
  collection: string;
  /** Documents to start from, one draft each — the clone sources. Each keeps its properties but
   * is given a fresh `_id`. Left out (or empty), the form opens on a single document holding
   * nothing but a newly minted `_id`. */
  seedDocs?: TypedDocument[];
  /** Mints `count` ids to prefill drafts with, following whatever scheme the collection is
   * keyed by. Consecutive within one call, so a form of several drafts asks once. */
  nextIds: (count: number) => Promise<TypedValue[]>;
  onCancel: () => void;
  /** Rejects with the reason the insert failed: the dialog then shows it and stays open with
   * the drafts still on screen. The caller is what closes the dialog, once this resolves. */
  onSubmit: (documents: TypedDocument[]) => Promise<void>;
}

/** How an `_id` compares to another for the duplicate check: two ids are the same when they are
 * the same BSON value, and every value here is plain JSON with its type spelled out. */
function idFingerprint(doc: TypedDocument): string | null {
  if (!("_id" in doc)) return null;
  return JSON.stringify(doc._id);
}

/** The first pair of drafts sharing an `_id`, as 1-based positions. Mongo would reject the
 * second of them partway through an insert the rest of which has already landed, so it is
 * worth catching before anything is written. */
function findDuplicateIds(documents: TypedDocument[]): { a: number; b: number } | null {
  const seen = new Map<string, number>();
  for (let i = 0; i < documents.length; i++) {
    const fingerprint = idFingerprint(documents[i]);
    // A draft with no `_id` is asking the server for one, and no two of those can collide.
    if (fingerprint === null) continue;
    const first = seen.get(fingerprint);
    if (first !== undefined) return { a: first + 1, b: i + 1 };
    seen.set(fingerprint, i);
  }
  return null;
}

/**
 * A form for writing new documents into a collection: one {@link Document} card per document,
 * each an editable tree rather than a row of cells — a document has no fixed set of columns to
 * lay out, so it is composed the same way an existing one is edited.
 *
 * Each card starts at nothing but its `_id`; every other property is added in the card. The
 * dialog holds no copy of what is being typed — the cards own their working copies and report
 * them up — so it only tracks which drafts exist.
 */
function InsertDocumentsDialog({ collection, seedDocs, nextIds, onCancel, onSubmit }: Props) {
  const { t } = useTranslation();
  const cloning = seedDocs !== undefined && seedDocs.length > 0;
  const [drafts, setDrafts] = useState<Draft[]>([]);
  const [preparing, setPreparing] = useState(true);
  const [addingDraft, setAddingDraft] = useState(false);
  const [errors, setErrors] = useState<string[]>([]);
  const [saving, setSaving] = useState(false);

  /** Each card's working copy, as last reported. Held in a ref rather than in state: it changes
   * on every commit inside a card, and nothing in this dialog renders from it — only the submit
   * reads it. */
  const docsRef = useRef<Map<number, TypedDocument>>(new Map());
  const nextKeyRef = useRef(0);
  // Props read by the mount effect below, which must run exactly once.
  const openRef = useRef({ nextIds, seedDocs });
  openRef.current = { nextIds, seedDocs };

  useEffect(() => {
    let cancelled = false;
    const { nextIds: mint, seedDocs: seeds } = openRef.current;
    const count = seeds && seeds.length > 0 ? seeds.length : 1;

    function build(ids: TypedValue[]): Draft[] {
      const sources: (TypedDocument | null)[] = seeds && seeds.length > 0 ? seeds : [null];
      return sources.map((source, i) => {
        const id = ids[i];
        // A clone keeps everything but the original's identity: its own `_id` goes first, and
        // the properties follow in the order they were stored in.
        const rest = source ? Object.fromEntries(Object.entries(source).filter(([k]) => k !== "_id")) : {};
        return {
          key: nextKeyRef.current++,
          seed: id === undefined ? rest : { _id: id, ...rest },
        };
      });
    }

    mint(count)
      .then((ids) => {
        if (!cancelled) setDrafts(build(ids));
      })
      .catch((e) => {
        if (cancelled) return;
        // Without ids the form still works: a document inserted with no `_id` gets one from the
        // server. Say why the field is missing rather than refusing to open.
        setErrors([errorMessage(t, e)]);
        setDrafts(build([]));
      })
      .finally(() => {
        if (!cancelled) setPreparing(false);
      });

    return () => {
      cancelled = true;
    };
  }, [t]);

  async function addDraft() {
    setAddingDraft(true);
    try {
      // Ids come from what the collection holds now, so asking twice hands back the same
      // numbers: ask for the whole form's worth and keep the one that is not spoken for.
      const ids = await nextIds(drafts.length + 1);
      const id = ids[ids.length - 1];
      setDrafts((prev) => [...prev, { key: nextKeyRef.current++, seed: id === undefined ? {} : { _id: id } }]);
    } catch (e) {
      setErrors([errorMessage(t, e)]);
    } finally {
      setAddingDraft(false);
    }
  }

  function removeDraft(key: number) {
    docsRef.current.delete(key);
    setDrafts((prev) => prev.filter((d) => d.key !== key));
    // The positions the recorded errors name have just moved.
    setErrors([]);
  }

  async function submit() {
    const documents = drafts.map((d) => docsRef.current.get(d.key) ?? d.seed);
    const duplicate = findDuplicateIds(documents);
    if (duplicate) {
      setErrors([t("insertDocuments.errorDuplicateId", duplicate)]);
      return;
    }
    setErrors([]);
    setSaving(true);
    try {
      await onSubmit(documents);
    } catch (e) {
      setErrors([errorMessage(t, e)]);
    } finally {
      setSaving(false);
    }
  }

  const busy = saving || preparing;

  return (
    <Modal
      label={collection}
      onClose={onCancel}
      locked={saving}
      size="large"
      title={t(cloning ? "insertDocuments.cloneTitle" : "insertDocuments.title", { collection })}
      actions={[
        { kind: "cancel", label: t("common.cancel"), disabled: saving },
        {
          kind: "confirm",
          label: t("insertDocuments.insert", { n: drafts.length }),
          onClick: () => void submit(),
          disabled: busy || drafts.length === 0,
          busy: saving ? t("insertDocuments.inserting") : undefined,
        },
      ]}
    >
      {() => (
        <>
          <ModalBody>
            <p className={styles.note}>{t("insertDocuments.idNote")}</p>
            <p className={styles.note}>{t("insertDocuments.orderedNote")}</p>

            <div className={styles.list}>
              {preparing && <p className="muted">{t("insertDocuments.preparing")}</p>}
              {drafts.map((d, i) => (
                <Document
                  key={d.key}
                  draft
                  doc={d.seed}
                  displayNumber={i + 1}
                  onChange={(doc) => docsRef.current.set(d.key, doc)}
                  // The last draft standing keeps no remove button: a form with nothing in it has
                  // nothing to insert, and Cancel is what closes it.
                  onRemove={drafts.length > 1 && !saving ? () => removeDraft(d.key) : undefined}
                />
              ))}
            </div>

            <div className={styles.toolbar}>
              <Button size="small" onClick={() => void addDraft()} disabled={busy || addingDraft}>
                <PlusIcon size={12} /> {t("insertDocuments.addDocument")}
              </Button>
            </div>
          </ModalBody>
          <ModalErrors messages={errors} />
        </>
      )}
    </Modal>
  );
}

export default InsertDocumentsDialog;
