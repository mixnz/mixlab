import { useCallback, useEffect, useState } from "react";
import { savedChange } from "../../../core/followSaved";
import { stableStringify } from "../../../core/stableStringify";
import { useTranslation } from "../../../i18n";
import { configFrom, formFrom, withKind, type ConnectionForm as FormState } from "./connectionForm";
import { addConnection, removeConnection, updateConnection } from "../saved/savedConnectionsStore";
import type { ConnectionConfig, DbKind, SavedConnection } from "../types";

interface Options {
  savedConnections: SavedConnection[];
  savedConnectionsLoaded: boolean;
  onTitleChange: (title: string) => void;
  onStateChange: (state: unknown) => void;
  /** Called whenever the form is replaced by another entry or a new form: what the connection was
   *  saying is about the one that was there before. */
  onReplaced: () => void;
}

/** What the connection form holds: the form itself, the name it is saved under, which saved entry
 *  it took, and whether that entry changed somewhere else since. */
export function useConnectionForm({
  savedConnections,
  savedConnectionsLoaded,
  onTitleChange,
  onStateChange,
  onReplaced,
}: Options) {
  const { t } = useTranslation();
  /* The whole form as one value — see `connectionForm.ts`, where the two directions live. What
     was seventeen `useState` calls, and two lists of seventeen setters that had to be kept in
     step by hand. */
  const [form, setForm] = useState(() => formFrom(null));

  /** One field of the form, changed. What the form component reports back through.
   *
   *  Editing `password` by hand clears `keyringRef`: what is now in the box is no longer what the
   *  reference points at, so Save must go back to writing a plain copy rather than the old
   *  address. */
  function set<K extends keyof FormState>(field: K, value: FormState[K]) {
    setForm((current) => ({
      ...current,
      [field]: value,
      ...(field === "password" ? { keyringRef: null } : {}),
    }));
  }

  const [saveAsName, setSaveAsName] = useState("");
  const [editingId, setEditingId] = useState<string | null>(null);
  const [savedSnapshot, setSavedSnapshot] = useState<string | null>(null);
  /** The saved connection this form took changed or went away somewhere else, while it held edits. */
  const [stale, setStale] = useState<"changed" | "removed" | null>(null);
  /** How many times the form has been asked to put the caret in the password field — see
   *  `ConnectionForm`'s `focusPassword`. Bumped for a handed-over connection that came without one. */
  const [focusPassword, setFocusPassword] = useState(0);

  function changeKind(next: DbKind) {
    setForm((current) => withKind(current, next));
  }

  /**
   * Wipes everything the form is saying back to the user. Each message is about the connection that
   * was in the form when it appeared — a failed connect, a tunnel test — so loading a different one
   * leaves it describing something that is no longer on screen.
   */
  function replaced() {
    onReplaced();
    setStale(null);
  }

  /** The saved entry again, over what the form holds: what sync brought, or another tab saved. The
   *  tab keeps what it is doing, so nothing here touches the connection or the restore state. */
  const reloadSaved = useCallback(
    (entry: SavedConnection) => {
      setForm(formFrom(entry.config, entry.keyringRef ?? null));
      setSaveAsName(entry.name);
      setSavedSnapshot(stableStringify({ name: entry.name, config: entry.config }));
      setStale(null);
      onTitleChange(entry.name);
    },
    [onTitleChange],
  );

  function applySaved(entry: SavedConnection) {
    setForm(formFrom(entry.config, entry.keyringRef ?? null));
    setEditingId(entry.id);
    setSaveAsName(entry.name);
    setSavedSnapshot(stableStringify({ name: entry.name, config: entry.config }));
    replaced();
    onTitleChange(entry.name);
    /* Beside the title, and for the same reason: from here on the tab is named after this
       connection, so next launch has to know which one that was. `connect` overwrites it a tick
       later when this is on the way in through `openAndConnect` — until then, and for a form the
       user is only looking at, the flag is honest about there being no connection open. */
    onStateChange({ savedId: entry.id, connected: false });
  }

  function newForm() {
    setEditingId(null);
    setSaveAsName("");
    setSavedSnapshot(null);
    setForm(formFrom(null));
    replaced();
    onTitleChange(t("app.newConnectionTitle"));
    // Nothing to point at any more, and the title says so too.
    onStateChange(undefined);
  }

  async function save() {
    const name = saveAsName.trim();
    if (!name) return;
    if (editingId) {
      // Everything the entry carries that the form doesn't — whether it is pinned, the sidebar
      // width, Redis's scan limit — is kept: saving a connection edits its settings, it doesn't
      // reset the rest of what is remembered about it.
      const existing = savedConnections.find((c) => c.id === editingId);
      const config = configFrom(form);
      const entry: SavedConnection = {
        ...existing,
        id: editingId,
        name,
        config,
        keyringRef: form.keyringRef ?? undefined,
      };
      await updateConnection(entry);
      setSavedSnapshot(stableStringify({ name: entry.name, config: entry.config }));
    } else {
      const entry: SavedConnection = {
        id: crypto.randomUUID(),
        name,
        config: configFrom(form),
        keyringRef: form.keyringRef ?? undefined,
      };
      await addConnection(entry);
      setEditingId(entry.id);
      setSavedSnapshot(stableStringify({ name: entry.name, config: entry.config }));
    }
  }

  async function saveAsNew() {
    const name = saveAsName.trim();
    if (!name) return;
    const entry: SavedConnection = {
      id: crypto.randomUUID(),
      name,
      config: configFrom(form),
      keyringRef: form.keyringRef ?? undefined,
    };
    await addConnection(entry);
    setEditingId(entry.id);
    setSavedSnapshot(stableStringify({ name: entry.name, config: entry.config }));
  }

  async function deleteSaved(id: string) {
    await removeConnection(id);
    if (editingId === id) {
      newForm();
    }
  }

  /** A connection another program handed over, put in the form. */
  function fill(config: ConnectionConfig, keyringRef: string | null, name: string) {
    setForm(formFrom(config, keyringRef));
    setSaveAsName(name);
  }

  const askForPassword = useCallback(() => setFocusPassword((count) => count + 1), []);
  const dismissStale = useCallback(() => setStale(null), []);

  /* The saved connection this form took can change under it: sync brings a newer one, or another
     tab saves or deletes it. An untouched form follows; edits are the person's and stay, and they
     are told. One that went away leaves the form as it is, and Save makes it a new one, rather than
     an update to an id that no longer exists, which would save nothing. */
  useEffect(() => {
    if (editingId === null || !savedConnectionsLoaded) return;
    const entry = savedConnections.find((c) => c.id === editingId);
    const current = entry && stableStringify({ name: entry.name, config: entry.config });
    const held = stableStringify({ name: saveAsName.trim(), config: configFrom(form) });
    const change = savedChange(current, savedSnapshot, held);
    if (change === "reload" && entry) reloadSaved(entry);
    else if (change === "changed") setStale("changed");
    else if (change === "removed") {
      setEditingId(null);
      setSavedSnapshot(null);
      setStale("removed");
    }
  }, [editingId, savedConnectionsLoaded, savedConnections, savedSnapshot, saveAsName, form, reloadSaved]);

  /** The saved entry the form holds, if it still exists. */
  const savedEntry = savedConnections.find((c) => c.id === editingId);

  /* A name is needed, and an edit that changes nothing is not a save. Worked out here
     rather than in the form, because it is a question about the saved list. */
  const saveDisabled =
    !saveAsName.trim() ||
    (editingId !== null &&
      savedSnapshot === stableStringify({ name: saveAsName.trim(), config: configFrom(form) }));

  return {
    form,
    set,
    changeKind,
    saveAsName,
    setSaveAsName,
    editingId,
    savedEntry,
    stale,
    dismissStale,
    focusPassword,
    askForPassword,
    saveDisabled,
    applySaved,
    newForm,
    reloadSaved,
    fill,
    save,
    saveAsNew,
    deleteSaved,
  };
}

export type ConnectionFormState = ReturnType<typeof useConnectionForm>;
