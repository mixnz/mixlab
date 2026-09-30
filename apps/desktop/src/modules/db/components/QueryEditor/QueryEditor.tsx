import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { columnDetail, completionSchema } from "../../sql/completion";
import {
  AUTO_LIMIT,
  isRowsDml,
  unguardedWrites,
  withAutoLimits,
  writingStatements,
  type UnguardedWrite,
} from "../../sql/guard";
import { lintScript, problemRange } from "../../sql/lint";
import { useSqlDialect } from "../../sql/context";
import { kindLabel } from "../../connectionForm";
import { referenceAt, type SqlReference } from "../../mysql/reference";
import { invalidateSchemaOutline, useSchemaOutline } from "../../sql/schemaCache";
import { useSqlApi } from "../../sql/context";
import {
  changesSchema,
  splitStatements,
  statementAt,
  type SqlStatement,
} from "../../sql/statements";
import ActionBar from "../../../../components/ActionBar";
import Button from "../../../../components/Button";
import ConfirmDialog from "../../../../components/ConfirmDialog";
import LoadingOverlay from "../../../../components/LoadingOverlay";
import SqlEditor, {
  type EditorCompletion,
  type EditorHover,
  type EditorLookup,
  type LintSources,
  type SqlEditorHandle,
} from "../SqlEditor";
import QueryHistoryDialog from "./QueryHistoryDialog";
import QueryResults from "./QueryResults";
import QuerySnippetsDialog from "./QuerySnippetsDialog";
import { useResultsPane } from "./resultsPane";
import { useResultsZoom } from "./resultsZoom";
import {
  BookmarkIcon,
  ChevronDownIcon,
  ChevronUpIcon,
  CloseIcon,
  ExpandIcon,
  FormatIcon,
  HistoryIcon,
  PlayIcon,
} from "../../../../icons";
import { useTranslation, type TranslationKey } from "../../../../i18n";
import { errorMessage } from "../../../../core/errors";
import { MODIFIER_LABEL, shortcutLabel } from "../../../../core/platform";
import { RELOAD_SHORTCUT, useReloadShortcut } from "../../../../core/reload";
import { loadDraft, saveDraft, saveDraftNow } from "../../queryDrafts";
import { recordQuery } from "../../queryHistory";
import { useQuerySnippets } from "../../querySnippets";
import type { SqlStatementResult } from "../../types";
import styles from "./QueryEditor.module.css";

/** How many of a table's columns the hover tooltip lists before it stops and counts the rest. A
 *  wide table would otherwise fill the window with something nobody is reading to the end of. */
const HOVER_COLUMNS = 14;

/** What a column's key flag is called. MySQL reports three, and an empty string for a column that
 *  leads no key at all. */
const KEY_LABELS: Readonly<Partial<Record<string, TranslationKey>>> = {
  PRI: "query.hoverPrimaryKey",
  UNI: "query.hoverUniqueKey",
  MUL: "query.hoverIndexed",
};

interface Props {
  connectionId: string;
  /** The database picked in the header. Applied as a `USE` before the script, so unqualified table
   *  names resolve the way they do in the other tabs. Empty means none is selected. */
  database: string;
  /** Whether this is what the user is actually looking at — the Query tab, in the connection tab
   *  the tab bar is showing. The tab stays mounted behind both so a script and its results survive
   *  a look at the data, but what completion reads is only worth reading for someone who is
   *  actually here, and `Ctrl+R` belongs to whichever pane that is. */
  active: boolean;
  /** The saved connection is marked as one nothing is written to, so a statement that would change
   *  anything is refused before it is sent. */
  readOnly?: boolean;
  /** See `SqlWorkspaceProps.dmlEvenIfReadOnly` — the same meaning, one level down. */
  dmlEvenIfReadOnly?: boolean;
  /** The *saved* connection's id, which survives the app closing — unlike `connectionId`, which is
   *  the session and is minted fresh on every connect. It is what the draft and the history are
   *  filed under. Empty for a connection nobody saved, and then neither is kept. */
  profileId?: string;
  /** Opens a table of the selected database elsewhere in the workspace — what `Ctrl+Click` on a
   *  table name in the script does. Absent means the script is read but never followed. */
  onOpenTable?: (table: string) => void;
  /**
   * Told when the script that just ran could have changed the selected database: `"schema"` when it
   * held a statement that changes what the tables are or how they are shaped, `"rows"` when it only
   * wrote rows.
   *
   * This tab is not the only pane over the database. The grid beside it is holding a page read before
   * the script ran, the Structure tab a set of columns, the Statistics tab a set of figures — and
   * after a `DROP COLUMN` run from here, that grid goes on drawing the dropped column and an edit in
   * it sends an `UPDATE` keyed on a column the table hasn't got. What to let go of is the
   * workspace's to decide, since the workspace is what holds all three.
   */
  onDatabaseChanged?: (change: "schema" | "rows") => void;
}

/**
 * A SQL editor over the connection, and what running it produced.
 *
 * What comes back is one result per statement, and each one is shown as what it is: a result set
 * becomes a read-only grid, a write reports how many rows it changed (and the id it generated),
 * and everything else reports that it ran. A statement that failed carries the reason instead, and
 * stops the ones after it — the results before it are still shown.
 *
 * There is one way to run — `Ctrl+R`, or the Run button that is the same thing — and what it sends
 * is decided by the selection: the selected text if there is any, the whole script if there is not.
 * The editor owns the selection, so it is the editor that answers the question.
 */
function QueryEditor({
  connectionId,
  database,
  active,
  readOnly = false,
  dmlEvenIfReadOnly = false,
  profileId = "",
  onOpenTable,
  onDatabaseChanged,
}: Props) {
  const { t, lang } = useTranslation();
  const api = useSqlApi();
  const dialect = useSqlDialect();
  const snippets = useQuerySnippets();
  /** The script, held in a ref rather than in state.
   *
   * The editor is the only thing that changes it and the only thing that draws it, so nothing here
   * needs to re-render when a character is typed — and nothing here *may*, either: the results
   * below are a thousand rows of tens of thousands of cells, and re-rendering those on every
   * keystroke is what made typing above a large result take seconds per key. What the toolbar
   * actually needs to know is only whether there is anything to run, and that is the state below.
   */
  const sql = useRef("");
  /** Whether the script has anything in it — the only thing about it the buttons ask about, and so
   *  the only thing worth a render. It changes twice per script, not once per keystroke. */
  const [hasSql, setHasSql] = useState(false);
  const [results, setResults] = useState<SqlStatementResult[] | null>(null);
  const [running, setRunning] = useState(false);
  /** Set once the server has been asked to stop the statement, until the script comes back. */
  const [cancelling, setCancelling] = useState(false);
  /** A failure to run the script at all — a lost connection, or nothing to run. Per-statement
   * errors are not this: they arrive inside the results and are shown against the statement. */
  const [error, setError] = useState("");
  /** How many statements of the last run were sent with a `LIMIT` they were not written with, so
   *  the results can say so instead of quietly showing fewer rows than were asked for. */
  const [limitsAdded, setLimitsAdded] = useState(0);
  /** How many statements the last run was split into. Held rather than counted from the results,
   *  because the point of it is the case where the two differ. */
  const [statementsSent, setStatementsSent] = useState(0);
  /** A run held up by the confirmation below: the text as it would be sent, and what is alarming
   *  about it. Null when nothing is waiting to be confirmed. */
  const [pending, setPending] = useState<{ text: string; writes: UnguardedWrite[] } | null>(null);
  const [historyOpen, setHistoryOpen] = useState(false);
  /** The query the snippets dialog is offering to keep, held from the moment the button was
   *  pressed: the caret moves while a name is being typed, and what is saved should not. */
  const [snippetSql, setSnippetSql] = useState<string | null>(null);
  const editorRef = useRef<SqlEditorHandle>(null);
  /** The results pane, which is also the box the button below lifts over the window — the same
   *  element either way, never a second copy of it. */
  const resultsRef = useRef<HTMLDivElement>(null);
  const zoom = useResultsZoom(resultsRef);
  /** The two halves the divider shares out, measured when it is dragged: how tall the results may
   *  be is a question about how much room there is, and the room is these two together. Not the
   *  tab — the toolbar and the bar at the bottom are in that, and neither is the divider's to
   *  spend. */
  const editorPaneRef = useRef<HTMLDivElement>(null);
  const slotRef = useRef<HTMLDivElement>(null);
  const pane = useResultsPane(editorPaneRef, slotRef);

  /** Whether this tab has ever been the one on screen.
   *
   * The editor measures itself against the DOM, and the tab is `display: none` until it is looked
   * at — one created in a box of no size lays itself out against no size. So it is created the
   * first time the tab is shown and kept from then on: before that there is nothing in it to
   * preserve, and afterwards the script and its results have to survive a look at the data. */
  const [shown, setShown] = useState(active);
  useEffect(() => {
    if (active) setShown(true);
  }, [active]);

  const outline = useSchemaOutline(api, connectionId, database, active);
  const schema = useMemo(() => completionSchema(outline, dialect), [outline, dialect]);

  /** The saved queries, as something the editor can offer. The detail is the query itself on one
   *  line: a name alone says what someone called it, not what it does. */
  const completions = useMemo<EditorCompletion[]>(
    () =>
      snippets.map((snippet) => ({
        label: snippet.name,
        detail: snippet.sql.replace(/\s+/g, " ").trim().slice(0, 80),
        apply: snippet.sql,
      })),
    [snippets]
  );

  /** Set while the draft for the pair named in the header is being fetched. Nothing is written back
   *  until it has been: the editor is emptied on the way in, and that empty document must not be
   *  saved over the very draft that is on its way out of the file. */
  const restoring = useRef(false);

  /**
   * The draft for this connection and database, put back where it was left.
   *
   * The order matters. The cleanup writes the outgoing draft at once rather than waiting for the
   * debounce — by the time that fired, this pair would no longer be the current one — and then the
   * editor is emptied before the new draft is read, so what is on screen always belongs to the
   * database named in the header. The read is asynchronous and the editor is usable throughout, so
   * a draft that arrives after something has been typed into that empty editor is dropped:
   * whatever is on screen is newer than whatever was on disk.
   */
  useEffect(() => {
    let current = true;
    restoring.current = true;
    sql.current = "";
    setHasSql(false);
    // Without focus: the database is changed from the header, and the pointer belongs there.
    editorRef.current?.setText("");
    void loadDraft(profileId, database)
      .then((draft) => {
        if (!current || draft === "" || sql.current !== "") return;
        sql.current = draft;
        setHasSql(/\S/.test(draft));
        editorRef.current?.setText(draft);
      })
      .catch(() => {})
      .finally(() => {
        if (current) restoring.current = false;
      });
    return () => {
      current = false;
      restoring.current = false;
      saveDraftNow(profileId, database, sql.current);
    };
  }, [profileId, database]);

  /** The last script split, kept so one keystroke costs one split. The editor asks for the caret's
   * statement as the script changes, and asks again for the same script when Run is pressed. */
  const split = useRef<{ doc: string; statements: SqlStatement[] } | null>(null);

  function statementsOf(doc: string): SqlStatement[] {
    if (split.current?.doc !== doc) split.current = { doc, statements: splitStatements(doc, dialect.syntax) };
    return split.current.statements;
  }

  /** Where the statement covering `pos` sits — what the editor draws its highlight over, so the
   * boundaries the server will split the script on are visible while it is written. Nothing runs
   * from it: `Ctrl+R` sends the selection or the lot. */
  const statementRange = useCallback((doc: string, pos: number) => {
    const statement = statementAt(doc, statementsOf(doc), pos);
    return statement && { from: statement.from, to: statement.to };
  }, []);

  /** Whether a script is in flight, where the error checker can read it. The checker is built once
   *  per schema and would otherwise be holding the value `running` had when it was built. */
  const busy = useRef(false);

  /** Which run Cancel is for. Minted per run rather than taken from the connection: a connection
   *  can be running a script for another editor, and a cancel naming only the connection would
   *  reach whichever of them started last. `null` between runs, which is why Cancel on nothing is
   *  not sent at all. */
  const runId = useRef<string | null>(null);

  /**
   * The two error checks, as the editor wants them.
   *
   * The instant one reads the whole script against the outline completion is already using; the
   * deep one asks the server about the statement being typed in. Both are rebuilt only when what
   * they answer from changes — the editor calls them, they do not call the editor.
   */
  const lint = useMemo<LintSources>(
    () => ({
      quick: (doc) =>
        lintScript(statementsOf(doc), outline, dialect).map((finding) => ({
          from: finding.from,
          to: finding.to,
          severity: finding.severity,
          message: t(finding.code, finding.params),
          ...(finding.suggestion
            ? {
                fix: {
                  label: t("lint.replaceWith", { name: finding.suggestion }),
                  text: finding.suggestion,
                },
              }
            : {}),
        })),
      deep: async (doc, pos) => {
        // With no database chosen there is nothing for the server to resolve names against, and
        // every statement would come back complaining about a table that is perfectly real. While
        // a script is running, the pool is better spent on the script.
        if (database === "" || busy.current) return [];
        const statement = statementAt(doc, statementsOf(doc), pos);
        if (!statement) return [];
        const problem = await api.validateSql(connectionId, statement.text, database);
        if (!problem) return [];
        return [
          {
            ...problemRange(statement, problem.line),
            severity: problem.severity,
            message: problem.message,
            // Named, so it reads as the server's opinion rather than as MixLab's — which matters
            // most for the warnings, where the server may simply be looking somewhere else.
            // The engine that actually answered: PostgreSQL's complaint attributed to MySQL is a
            // worse label than none, and the same goes for a third engine attributed to either.
            source: t(kindLabel(dialect.kind)),
          },
        ];
      },
    }),
    [outline, api, connectionId, database, t]
  );

  /**
   * What the name under the pointer is, said in words.
   *
   * The resolving is `referenceAt`'s, which reads an alias the way the checks above read one — so
   * the tooltip over `u.id` and the warning under it can never disagree about which table `u` is.
   * All that happens here is the turning of its answer into sentences, which is this layer's job
   * because this is the layer that knows which language the app is set to.
   */
  const lookup = useMemo<EditorLookup>(() => {
    const find = (doc: string, pos: number) => referenceAt(statementsOf(doc), pos, outline, dialect);

    function describe(reference: SqlReference): EditorHover {
      const at = { from: reference.from, to: reference.to };

      if (reference.kind === "function") {
        return { ...at, title: reference.signature, subtitle: t("query.hoverFunction") };
      }

      // A column says what it holds, whether it may be empty, what it keys and where it points —
      // which is the whole of what the outline knows about one.
      if (reference.kind === "column") {
        const { column } = reference;
        const key = KEY_LABELS[column.key];
        const parts = [
          column.dataType,
          column.nullable ? t("query.hoverNullable") : t("query.hoverNotNull"),
          ...(key ? [t(key)] : []),
          // An arrow needs no translating, and the target is a name rather than a word. Spelled
          // `->` for the reason `columnDetail` spells it that way: the bundled font draws it.
          ...(column.references ? [`-> ${column.references}`] : []),
        ];
        return {
          ...at,
          title: `${reference.table.name}.${column.name}`,
          subtitle: parts.join(" · "),
        };
      }

      const { columns } = reference.table;
      const footer = [
        columns.length > HOVER_COLUMNS
          ? t("query.hoverMoreColumns", { n: columns.length - HOVER_COLUMNS })
          : null,
        onOpenTable ? t("query.hoverJump", { shortcut: shortcutLabel("Click") }) : null,
      ].filter((part) => part !== null);
      return {
        ...at,
        title: reference.table.name,
        subtitle:
          columns.length === 1
            ? t("query.hoverTableOne")
            : t("query.hoverTable", { n: columns.length }),
        items: columns.slice(0, HOVER_COLUMNS).map((column) => ({
          name: column.name,
          detail: columnDetail(column),
        })),
        ...(footer.length > 0 ? { footer: footer.join(" · ") } : {}),
      };
    }

    return {
      hover: (doc, pos) => {
        const reference = find(doc, pos);
        return reference ? describe(reference) : null;
      },
      // Only a table leads anywhere. A column's own table is one hover away and one click further,
      // and a name that opened a tab for its table would surprise whoever aimed at the column.
      target: (doc, pos) => {
        if (!onOpenTable) return null;
        const reference = find(doc, pos);
        if (reference?.kind !== "table") return null;
        const { name } = reference.table;
        return { from: reference.from, to: reference.to, open: () => onOpenTable(name) };
      },
    };
  // `lang` beside `t`: `t` is one function for the life of the app now, so it is `lang` that says
  // this holds words and has to be built again when they change. See `i18n/index.tsx`.
  }, [outline, onOpenTable, t, lang]);

  /** Every keystroke lands here, and almost every one of them ends without a render: React drops a
   *  state write that changes nothing, and this one changes twice in a script's life.
   *
   *  Tested with a regex rather than `trim()`, which on a large script copies the whole thing to
   *  answer a question that the first non-space character already settles. */
  const editorChanged = useCallback(
    (text: string) => {
      sql.current = text;
      setHasSql(/\S/.test(text));
      // Not while the draft is still being read: the first change after a database switch is the
      // editor being emptied by the effect above, and saving that would delete the draft this very
      // moment is spent fetching.
      if (!restoring.current) saveDraft(profileId, database, text);
    },
    [profileId, database]
  );

  /**
   * The gate every Run press goes through.
   *
   * Three checks, in the order of how final they are: a read-only connection refuses outright, an
   * unguarded write stops and asks, and everything else goes straight on. Only the middle one can
   * be overridden, and only by the person reading what it says.
   *
   * Split afresh rather than through the memo above: what runs is usually one statement out of the
   * script, and it is the whole script that memo is there to hold on to.
   */
  function requestRun(text: string) {
    if (text.trim() === "" || running) return;
    const statements = splitStatements(text, dialect.syntax);

    if (readOnly) {
      const writes = writingStatements(statements, dialect).filter(
        (w) => !(dmlEvenIfReadOnly && isRowsDml(w.statement, dialect))
      );
      if (writes.length > 0) {
        setResults(null);
        setError(
          t(dmlEvenIfReadOnly ? "query.ddlBlocked" : "query.readOnlyBlocked", { verb: writes[0].verb })
        );
        // The refusal is shown in the pane, so it has to be a pane that is up.
        pane.reveal();
        return;
      }
    }

    const writes = unguardedWrites(statements, dialect);
    if (writes.length > 0) {
      setPending({ text, writes });
      return;
    }
    void run(text, statements);
  }

  /** What the Run button and `Ctrl+R` both do: whatever the editor says is pointed at — the
   *  selection, or the whole script when there is none. `sql.current` is the fallback for the
   *  moment before the editor exists, which is only ever the very first press. */
  const runRequested = () => requestRun(editorRef.current?.textToRun() || sql.current);

  // The Query tab's answer to the shortcut every other pane answers with its reload button. The
  // guard inside `requestRun` is what stops a second press while a script is in flight.
  //
  // Never from behind a dialog. The history and the snippets have the keyboard while they are open,
  // and the unguarded-write question is the sharpest case of all: the key that opened it would send
  // the very script it is asking about, around the answer it is waiting for.
  useReloadShortcut(
    active && !historyOpen && snippetSql === null && pending === null,
    runRequested
  );

  async function run(text: string, statements: SqlStatement[]) {
    busy.current = true;
    const thisRun = crypto.randomUUID();
    runId.current = thisRun;
    // Whatever the pane was doing, a run is a request to see what comes back.
    pane.reveal();
    setRunning(true);
    setCancelling(false);
    setError("");
    // What is sent, which is not always what is on screen: a `SELECT` with no ceiling of its own
    // gets one, and the results say how many statements that happened to.
    const { sql: sent, added } = withAutoLimits(text, statements, AUTO_LIMIT, dialect);
    setLimitsAdded(added);
    setStatementsSent(statements.length);

    const startedAt = Date.now();
    // Remembered as the user wrote it, not as it was sent: a `LIMIT` MixLab added is not part of
    // the query they would want back.
    /* Nothing is remembered for a connection nobody saved. History is filed under the saved
       connection's id, so an unsaved one wrote every run under `""`: a row the dialog never
       shows, that Clear history cannot reach, and that still takes one of the 300 places from
       the runs someone can actually see. */
    const remember = (rowCount: number | null, failure: string | null) =>
      profileId === ""
        ? undefined
        : recordQuery({
            sql: text,
            profileId,
            database,
            startedAt,
            durationMs: Date.now() - startedAt,
            rowCount,
            error: failure,
          });

    try {
      const produced = await api.runScript(connectionId, thisRun, sent, database || undefined);
      setResults(produced);
      // The last result set is the one on screen when the script finishes, so it is the one worth
      // counting. A statement that failed stops the script, so at most the last carries a reason.
      const sets = produced.filter((result) => result.kind === "rows");
      const last = sets[sets.length - 1];
      // A set the backend stopped decoding holds its ceiling rather than its size, and recording
      // that as the count would put "1000 rows" in the history for an answer that had far more.
      // Nothing is claimed instead: the history says what was asked, and this is not known.
      const rowCount = last && !last.truncated ? last.rows.length : null;
      remember(rowCount, produced[produced.length - 1]?.error ?? null);
      // Completion is working from a copy of the schema, and a script that has just created or
      // dropped something has made that copy wrong. The panes over the same database are told as
      // well, and told which of the two kinds of change it was — see {@link Props.onDatabaseChanged}.
      //
      // Whether the script succeeded is not asked: a statement that failed stops the ones after it,
      // but the ones before it ran, and a script half-applied is exactly the case where what the
      // other panes are showing must not be trusted. Anything the read-only guard would have refused
      // counts as having written rows, which is generous — a `SET` changes nothing anyone is looking
      // at — and the cost of being generous is one re-read of a tab nobody may even open.
      if (database !== "" && changesSchema(statements)) {
        invalidateSchemaOutline(connectionId, database);
        onDatabaseChanged?.("schema");
      } else if (database !== "" && writingStatements(statements, dialect).length > 0) {
        onDatabaseChanged?.("rows");
      }
    } catch (e) {
      const message = errorMessage(t, e);
      setError(message);
      setResults(null);
      remember(null, message);
    } finally {
      busy.current = false;
      runId.current = null;
      setRunning(false);
      setCancelling(false);
    }
  }

  /** What the confirmation says. One statement is named; several are counted and listed, because a
   *  sentence about each of five of them is a sentence nobody reads to the end of.
   *
   *  A dropped table and a rewritten one are asked about in different words: "every row" is not
   *  what is at stake when the table itself is going. A script holding both is asked about in the
   *  words that cover both. */
  function unguardedMessage(writes: UnguardedWrite[]): string {
    if (writes.length === 1) {
      const [only] = writes;
      if (only.kind === "drop") {
        return only.table === ""
          ? t("query.unguardedDropUnnamed", { verb: only.verb })
          : t("query.unguardedDrop", { verb: only.verb, table: only.table });
      }
      return only.table === ""
        ? t("query.unguardedOneUnnamed", { verb: only.verb })
        : t("query.unguardedOne", { verb: only.verb, table: only.table });
    }
    const list = writes.map((write) => `${write.verb} ${write.table}`.trim()).join(", ");
    return writes.every((write) => write.kind === "rows")
      ? t("query.unguardedMany", { n: writes.length, list })
      : t("query.unguardedManyMixed", { n: writes.length, list });
  }

  /** Asks the server to stop the statement in flight. The script itself still returns through
   * {@link run} — with the killed statement carrying the server's reason — so there is nothing to
   * do here but say that it has been asked for. */
  async function cancel() {
    const run = runId.current;
    if (!running || cancelling || run === null) return;
    setCancelling(true);
    try {
      await api.cancelQuery(connectionId, run);
    } catch (e) {
      setError(errorMessage(t, e));
      setCancelling(false);
    }
  }

  /** Whether there is anything for the pane to hold. The tab opens on nothing but the editor —
   *  there is no result to show and no room worth spending on saying so — and this turns true the
   *  moment the script is sent, so what the pane rises with is the running veil rather than an
   *  empty frame. */
  const hasResults = running || results !== null || error !== "";

  /** Whether it is standing up: something to show, and not put away by the button in the footer. */
  const showResults = hasResults && !pane.shut;

  return (
    <div className={styles.queryEditor}>
      <div className={styles.toolbar}>
        {/* The only Run there is. What it runs is decided by the selection, not by a second
            button: the whole script, or exactly the text picked out of it. */}
        <Button
          size="small"
          variant="primary"
          title={`${RELOAD_SHORTCUT} — ${t("query.selectionHint")}`}
          onClick={runRequested}
          disabled={running || !hasSql}
        >
          <PlayIcon size="0.9em" />
          {running ? t("query.running") : t("query.run")}
        </Button>
        <Button
          size="small"
          title={shortcutLabel("F", { shift: true })}
          onClick={() => editorRef.current?.format()}
          disabled={!hasSql}
        >
          <FormatIcon size="0.9em" />
          {t("query.format")}
        </Button>
        {/* Never disabled: the dialog is where a saved query is dropped as well as where one is
            kept, and an empty editor is exactly when someone wants to go and fetch one. */}
        <Button
          size="small"
          title={t("query.snippetHint")}
          onClick={() => setSnippetSql(editorRef.current?.textToRun() || sql.current)}
        >
          <BookmarkIcon size="0.9em" />
          {t("query.snippets")}
        </Button>
        {/* Only for a saved connection: there would be nothing to file the history under
            otherwise, so there is nothing to open. */}
        {profileId !== "" && (
          <Button size="small" onClick={() => setHistoryOpen(true)}>
            <HistoryIcon size="0.9em" />
            {t("query.history")}
          </Button>
        )}
        {/* Absent, not disabled, on an engine with nothing to cancel through: SQLite runs the
            statement in this process against a file, so there is no session to reach in and stop.
            A button that could never do anything is worse than no button. */}
        {running && dialect.cancellable && (
          <Button size="small" onClick={() => void cancel()} disabled={cancelling}>
            {cancelling ? t("query.cancelling") : t("query.cancel")}
          </Button>
        )}
        {/* Beside the target, because it is part of the same sentence: which server, and what may
            be done to it. */}
        {readOnly && (
          <span
            className={styles.readOnly}
            title={t(dmlEvenIfReadOnly ? "query.ddlOnlyReadOnlyHint" : "common.readOnlyConnection")}
          >
            {t(dmlEvenIfReadOnly ? "query.ddlOnlyReadOnly" : "query.readOnly")}
          </span>
        )}
        {database ? (
          <span className={styles.target}>
            <span className={styles.targetLabel}>{t("query.targetLabel")}</span>
            <span className={styles.targetName}>{database}</span>
          </span>
        ) : (
          // Dashed and unaccented: there is nothing to point at yet, and what to do about it is
          // one hover away rather than a second line of text in the chip.
          <span className={`${styles.target} ${styles.targetEmpty}`} title={t("query.noDatabaseHint")}>
            {t("query.noDatabase")}
          </span>
        )}
      </div>

      {/* The editor and the strip naming it are one framed surface: the tab opens on an empty
          editor, and the frame is what tells you where the script goes. */}
      <div ref={editorPaneRef} className={styles.editorPane}>
        <div className={styles.editorBar}>
          <span>{t("query.editorHeading")}</span>
          {/* One shortcut, because there is one way to run: what it sends is chosen by selecting,
              not by picking a different key. The tooltip spells that out. */}
          <span className={styles.editorHints} title={t("query.selectionHint")}>
            <kbd className={styles.key}>{MODIFIER_LABEL}</kbd>
            <kbd className={styles.key}>R</kbd>
            <span>{t("query.runShortcutHint")}</span>
          </span>
        </div>
        <div className={styles.editorHost}>
          {shown && (
            <SqlEditor
              ref={editorRef}
              // Read once, as the editor is created — which is the first time this tab is looked
              // at, by which point the draft may already have been fetched into the ref.
              initialValue={sql.current}
              onChange={editorChanged}
              schema={schema}
              database={database}
              dialect={dialect.cmDialect}
              statementRange={statementRange}
              lint={lint}
              lookup={lookup}
              completions={completions}
              placeholder={t("query.placeholder")}
              ariaLabel={t("query.editorLabel")}
            />
          )}
        </div>
      </div>

      {historyOpen && (
        <QueryHistoryDialog
          profileId={profileId}
          // With focus: the dialog is closing, and the caret belongs at the end of what it put back.
          onPick={(picked) => editorRef.current?.setText(picked, true)}
          onClose={() => setHistoryOpen(false)}
        />
      )}

      {snippetSql !== null && (
        <QuerySnippetsDialog
          sql={snippetSql}
          onPick={(picked) => editorRef.current?.setText(picked, true)}
          onClose={() => setSnippetSql(null)}
        />
      )}

      {pending && (
        <ConfirmDialog
          // The stronger of the two questions when the script holds both: what is being asked
          // about is the worst thing in it, not the first.
          title={
            pending.writes.some((write) => write.kind === "drop")
              ? t("query.unguardedDropTitle")
              : t("query.unguardedTitle")
          }
          message={unguardedMessage(pending.writes)}
          confirmLabel={t("query.unguardedConfirm")}
          danger
          onConfirm={() => {
            const { text } = pending;
            setPending(null);
            void run(text, splitStatements(text, dialect.syntax));
          }}
          onCancel={() => setPending(null)}
        />
      )}

      {/* The backdrop the lifted box stands on. Portalled, and the only thing here that is: the box
          itself stays exactly where it is in the tree, so lifting it costs no re-render of the
          thousands of cells inside it. */}
      {zoom.zoomed &&
        createPortal(
          <div
            className={
              zoom.leaving ? `${styles.zoomVeil} ${styles.zoomVeilOut}` : styles.zoomVeil
            }
            onMouseDown={zoom.close}
          />,
          document.body
        )}

      {/* Only where there is something on both sides of it to divide. A tab that has not been run
          has one pane, and a handle for splitting one pane is a handle for nothing. */}
      {showResults && (
        <div
          className={pane.dragging ? `${styles.divider} ${styles.dividerHeld}` : styles.divider}
          role="separator"
          aria-orientation="horizontal"
          aria-label={t("query.resizeResults")}
          aria-valuenow={pane.height}
          tabIndex={0}
          {...pane.divider}
        />
      )}

      {/* The room the results are given, and the only thing here whose height changes: the pane
          inside is laid out at its full size from the first frame and pinned to the bottom edge, so
          growing this uncovers it from the bottom up instead of laying out a thousand rows again on
          every frame of the movement. */}
      <div
        ref={slotRef}
        className={[
          styles.resultsSlot,
          showResults ? "" : styles.resultsSlotShut,
          pane.dragging ? styles.resultsSlotHeld : "",
        ]
          .filter(Boolean)
          .join(" ")}
        style={{ height: showResults ? pane.height : 0 }}
        aria-hidden={!showResults}
      >
        <div className={styles.resultsPanel} style={{ height: pane.height }}>
          <div
            ref={resultsRef}
            className={
              zoom.zoomed ? `${styles.resultsWrap} ${styles.resultsZoomed}` : styles.resultsWrap
            }
          >
            {/* Only while it is up: down in the tab the pane needs no title, since what is under
                the editor is obviously what the editor produced. */}
            {zoom.zoomed && (
              <header className={styles.zoomBar}>
                <span>{t("query.zoomTitle")}</span>
                <button
                  type="button"
                  className={styles.historyClose}
                  onClick={zoom.close}
                  title={t("common.close")}
                  aria-label={t("common.close")}
                >
                  <CloseIcon />
                </button>
              </header>
            )}
            <QueryResults
              results={results}
              error={error}
              limitsAdded={limitsAdded}
              limit={AUTO_LIMIT}
              statementsSent={statementsSent}
            />
            {running && (
              <LoadingOverlay label={cancelling ? t("query.cancelling") : t("query.running")} />
            )}
          </div>
        </div>
      </div>

      {/* The tab's own bar, in the same place and the same shape the data and document tabs put
          theirs. It stays put whether or not there is a result above it — it is a fixture of the
          tab rather than part of the pane that comes and goes, and what it holds says for itself
          when there is nothing to act on. */}
      <div className={styles.footer}>
        <ActionBar
          actions={[
            {
              key: "results",
              // Points the way the pane will move: down to put it away, up to bring it back.
              icon: showResults ? ChevronDownIcon : ChevronUpIcon,
              label: showResults ? t("query.hideResults") : t("query.showResults"),
              // Out while the box is up there. The veil already covers this bar, so a pointer
              // cannot reach the button at all — this is what makes the keyboard agree, and it
              // spares the descent that would otherwise fly the box home to a slot closing under
              // it.
              disabled: !hasResults || zoom.zoomed,
              disabledHint: zoom.zoomed ? t("query.resultsZoomed") : t("query.resultsEmpty"),
              onClick: pane.toggle,
            },
            {
              key: "zoom",
              icon: ExpandIcon,
              label: t("query.zoom"),
              // Nothing to lift is not the same as a button that does nothing: an icon with no
              // text has to say why it is grey. Results put away are nothing to lift either —
              // lifting them would take them from a slot that is not on screen, and put them back
              // into it.
              disabled: results === null || results.length === 0 || pane.shut,
              disabledHint: pane.shut ? t("query.zoomShut") : t("query.zoomEmpty"),
              onClick: zoom.open,
            },
          ]}
        />
      </div>
    </div>
  );
}

export default QueryEditor;
