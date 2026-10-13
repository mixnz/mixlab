import { useEffect, useEffectEvent, useImperativeHandle, useLayoutEffect, useRef, type Ref } from "react";
import type { SQLDialect, SQLNamespace } from "@codemirror/lang-sql";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { format } from "sql-formatter";
import {
  docText,
  editorSetup,
  schemaCompartment,
  sqlLanguage,
  type EditorCompletion,
  type StatementRange,
} from "./extensions";
import { requestRecheck, type LintSources } from "./lint";
import type { EditorLookup } from "./lookup";
import styles from "./SqlEditor.module.css";

/** What the toolbar around the editor can ask it for. The editor owns the caret and the selection,
 *  so only it can say what a Run button would actually run. */
export interface SqlEditorHandle {
  /** The selection when there is one, else the whole script. */
  textToRun: () => string;
  /** Replaces the whole script — a draft being restored, or a query taken out of the history. The
   *  only way in: the editor owns the document while it is mounted.
   *
   *  `focus` moves the caret into the editor as well, which is what a query picked out of a dialog
   *  wants and what a draft swapped in under the tab does not — changing the database in the header
   *  should leave the pointer where it was. */
  setText: (text: string, focus?: boolean) => void;
  format: () => void;
  focus: () => void;
}

interface Props {
  /** What the editor opens with, read once as it is created. It is **not** watched afterwards: the
   *  document belongs to CodeMirror from that moment on, and everything that would replace it goes
   *  through {@link SqlEditorHandle.setText}. */
  initialValue: string;
  onChange: (value: string) => void;
  /** Tables and columns to complete against, or null while there are none to offer. */
  schema: SQLNamespace | null;
  /** The database the schema describes, so `db.table` completes as well as `table`. */
  database: string;
  /** The engine this script is written against: what is painted as a keyword, and what the
   *  built-in completions offer. Handed in rather than reached for, so this component stays a
   *  plain editor with no opinion about connections. */
  dialect: SQLDialect;
  /** Where the statement covering `pos` sits. Supplied by the caller: what splits a script into
   *  statements is a property of the database, not of the editor. */
  statementRange: (doc: string, pos: number) => StatementRange | null;
  /** Where the error underlines come from, or null for an editor that checks nothing. */
  lint: LintSources | null;
  /** What the names in the script mean — the hover tooltip, and where `Ctrl+Click` goes. Null for
   *  an editor with nothing to look them up in. */
  lookup: EditorLookup | null;
  /** Offered alongside the schema's tables and columns. */
  completions: readonly EditorCompletion[];
  placeholder: string;
  ariaLabel: string;
  ref?: Ref<SqlEditorHandle>;
}

/**
 * A SQL editor: syntax highlighting in the engine's own dialect, completion over the database's
 * own tables and columns, search and replace, and the shortcuts a script is worked through with.
 *
 * CodeMirror owns the document while the editor is mounted, and the bridge is deliberately narrow:
 * `initialValue` is read as it is created, everything typed comes back out through `onChange`, and
 * anything that would replace the script from outside goes in through `setText`. There is no
 * watched `value` prop, on purpose — a document mirrored into a React render is a document that can
 * be written back from a render made before the last keystroke, which loses the keystroke and takes
 * the caret with it.
 *
 * The extension list is built once. Callbacks reach it through a ref rather than through a rebuild,
 * and the one thing that does change — the schema — is swapped through a compartment.
 */
function SqlEditor({
  initialValue,
  onChange,
  schema,
  database,
  dialect,
  statementRange,
  lint,
  lookup,
  completions,
  placeholder,
  ariaLabel,
  ref,
}: Props) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  /** The script the editor being torn down had, so a rebuild opens on what was on screen rather
   *  than on what React last rendered — the two part company as soon as a key is pressed, since a
   *  keystroke does not necessarily cost a render. Null until there has been an editor to carry
   *  anything from. */
  const carried = useRef<string | null>(null);
  /** The current props, for the extensions that were built once and outlive this render. */
  const latest = useRef({
    onChange,
    statementRange,
    schema,
    database,
    dialect,
    lint,
    lookup,
    completions,
  });

  useLayoutEffect(() => {
    latest.current = {
      onChange,
      statementRange,
      schema,
      database,
      dialect,
      lint,
      lookup,
      completions,
    };
  });

  /** The selection, or the whole script when there is none.
   *
   * Selecting is the only way to run part of a script — the caret's own statement is not a third
   * answer. It was one once, under `Ctrl+Enter`, and it made the same keystroke mean two different
   * amounts of work depending on where the caret happened to be resting. */
  function textToRun(): string {
    const current = view.current;
    if (!current) return "";
    const selection = current.state.selection.main;
    return selection.empty
      ? docText(current.state.doc)
      : current.state.sliceDoc(selection.from, selection.to);
  }

  /** Reformats the selection, or the whole script when there is none. Text that cannot be parsed
   *  is left exactly as it was typed: a formatter is a convenience, not a gate. */
  function formatDocument() {
    const current = view.current;
    if (!current) return;
    const selection = current.state.selection.main;
    const from = selection.empty ? 0 : selection.from;
    const to = selection.empty ? current.state.doc.length : selection.to;
    const source = current.state.sliceDoc(from, to);
    if (source.trim() === "") return;

    let formatted: string;
    try {
      formatted = format(source, {
        language: "mysql",
        tabWidth: 2,
        keywordCase: "upper",
        linesBetweenQueries: 1,
      });
    } catch {
      return;
    }
    if (formatted === source) return;

    current.dispatch({
      changes: { from, to, insert: formatted },
      // The caret cannot be kept where it was — the text around it has moved — so it is put at the
      // end of what was reformatted, which is where reading resumes.
      selection: { anchor: from + formatted.length },
    });
    current.focus();
  }

  function setText(text: string, focus = false) {
    const current = view.current;
    if (!current) return;
    current.dispatch({
      changes: { from: 0, to: current.state.doc.length, insert: text },
      selection: { anchor: text.length },
    });
    if (focus) current.focus();
  }

  useImperativeHandle(ref, () => ({
    textToRun,
    setText,
    format: formatDocument,
    focus: () => view.current?.focus(),
  }));

  // What the editor opens with. An effect event because `initialValue` is what to open with, not
  // something to keep up with: rebuilding the editor around it would throw away the undo history
  // every time the script changed.
  const openingText = useEffectEvent(() => carried.current ?? initialValue);

  // Built once. `placeholder` and `ariaLabel` are in the deps because they are translated strings:
  // switching language rebuilds the editor, which costs the undo history and nothing else.
  useEffect(() => {
    const parent = host.current;
    if (!parent) return;

    const state = EditorState.create({
      // What the last editor had, if there was one. Only a language switch rebuilds this, and it
      // costs the undo history — it must not also cost the script.
      doc: openingText(),
      extensions: [
        editorSetup({
          placeholder,
          // Whatever has been loaded by now. A rebuild — which only a language switch causes —
          // must not drop the schema on the floor: the effect below reconfigures on a *change*,
          // and the schema has not changed, only the editor under it.
          schema: latest.current.schema,
          database: latest.current.database,
          dialect: latest.current.dialect,
          statementRange: (doc, pos) => latest.current.statementRange(doc, pos),
          onFormat: formatDocument,
          lint: () => latest.current.lint,
          lookup: () => latest.current.lookup,
          completions: () => latest.current.completions,
        }),
        EditorView.contentAttributes.of({ "aria-label": ariaLabel }),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) latest.current.onChange(docText(update.state.doc));
        }),
      ],
    });

    const created = new EditorView({ state, parent });
    view.current = created;
    return () => {
      carried.current = docText(created.state.doc);
      created.destroy();
      view.current = null;
    };
  }, [placeholder, ariaLabel]);

  // The schema arrives after the editor does — one read of the database, a moment behind the tab
  // opening — and is swapped in without touching anything else.
  useEffect(() => {
    const current = view.current;
    if (!current) return;
    current.dispatch({ effects: schemaCompartment.reconfigure(sqlLanguage(schema, database, dialect)) });
    // The checks were made against whatever was known a moment ago — which, until now, was
    // nothing. Left alone they would keep underlining every table in the script as unknown until
    // the next keystroke.
    requestRecheck(current);
  }, [schema, database, dialect]);

  return <div className={styles.editor} ref={host} />;
}

export default SqlEditor;
