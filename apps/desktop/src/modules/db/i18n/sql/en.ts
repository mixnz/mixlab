/**
 * The SQL workspace's strings: its Structure and Statistics tabs and their dialogs, the Query tab,
 * lint, the filter bar, the grid and inserting rows.
 *
 * Spread into `../en.ts` where these groups stood, so every `t("...")` still finds them by
 * name. Plain data, importing nothing.
 */
const sqlEn = {
  sql: {
    noTables: "No tables",
    noMatchingTables: "No matching tables",
    serverInfo: "{{os}} \u00b7 {{engine}} {{version}}",
    databaseLabel: "Database",
    databasePlaceholder: "(none)",
    searchDatabasesPlaceholder: "Search databases...",
    createDatabase: "Create a database",
    reloadDatabases: "Reload databases",
    dataTab: "Data",
    structureTab: "Structure",
    statsTab: "Statistics",
    queryTab: "Query",
    searchTablesPlaceholder: "Search tables...",
    followedTableHint: "Opened by following a foreign key. It stays here until you pick another table",
    reloadTables: "Reload tables",
    addTable: "Create a table",
    addTableSystem: "{{database}} belongs to the server, so no table can be added to it",
    systemTable: "{{database}} belongs to the server, so its tables cannot be renamed or dropped",
    renameTable: "Rename",
    dropTable: "Drop table",
    renameTableTitle: "Rename {{table}}",
    dropTableTitle: "Drop table?",
    dropTableMessage: "Drop {{table}} and every row in it? This cannot be undone.",
    resizeSidebar: "Resize sidebar",
    resizeSidebarTooltip: "Drag to resize, double-click to fit",
    selectTablePrompt: "Select a table to view its data.",
    selectTableStructurePrompt: "Select a table to view its structure.",
    selectDatabaseStatsPrompt: "Select a database to see what its tables weigh.",
  },
  // The Structure tab: a table's columns above, its indexes below. Every change is one ALTER TABLE
  // of its own, so the wording talks about single columns and indexes rather than pending edits.
  structure: {
    columnsTitle: "Columns",
    indexesTitle: "Indexes",
    reload: "Reload structure",
    loading: "Loading...",
    saving: "Applying...",
    addColumn: "Add a column",
    editColumn: "Edit this column",
    dropColumn: "Drop this column",
    noColumns: "No columns.",
    filterColumns: "Search columns...",
    noColumnsMatch: "No column matches this search.",
    colName: "Name",
    colType: "Type",
    colNullable: "Null",
    colDefault: "Default",
    colExtra: "Extra",
    colCollation: "Collation",
    colComment: "Comment",
    yes: "YES",
    no: "NO",
    none: "—",
    primaryTooltip: "Part of the primary key",
    uniqueTooltip: "The leading column of a unique index",
    indexTooltip: "The leading column of an index",
    autoIncrementTooltip: "The server assigns this value itself",
    expressionTooltip: "An expression the server runs for each new row, not text it stores",
    generatedTooltip:
      "A generated column. Its expression is not read here, so it can only be dropped",
    dropColumnTitle: "Drop column?",
    dropColumnMessage: "Drop {{column}} and everything stored in it? This cannot be undone.",
    addIndex: "Add an index",
    editIndex: "Edit this index",
    dropIndex: "Drop this index",
    noIndexes: "No indexes.",
    indexName: "Name",
    indexKind: "Kind",
    indexMethod: "Method",
    indexColumns: "Columns",
    indexComment: "Comment",
    indexExpression: "(expression)",
    kindPrimary: "PRIMARY",
    kindUnique: "UNIQUE",
    kindFulltext: "FULLTEXT",
    kindSpatial: "SPATIAL",
    kindIndex: "INDEX",
    functionalIndexTooltip:
      "This index is on an expression, which is not read here, so it can only be dropped",
    dropIndexTitle: "Drop index?",
    dropIndexMessage: "Drop the index {{index}}? The rows themselves are untouched.",
    skipIndexesTitle: "Skip indexes",
    addSkipIndex: "Add a skip index",
    noSkipIndexes: "No skip indexes.",
    skipIndexName: "Name",
    skipIndexExpr: "Expression",
    skipIndexType: "Type",
    skipIndexGranularity: "Granularity",
    dropSkipIndexTitle: "Drop this skip index?",
    dropSkipIndexMessage:
      'This drops the skip index "{{index}}". It only speeds up scanning, so no data is lost.',
    rebuildEngineNotAllowed:
      "MixLab has only verified the sorting-key rebuild against MergeTree, ReplacingMergeTree, SummingMergeTree and AggregatingMergeTree. This table's engine is {{engine}}.",
  },
  // The Statistics tab, shared by both workspaces: what every table or collection of the selected
  // database weighs. MySQL counts rows in tables, MongoDB documents in collections, so the three
  // headings that name one or the other come in pairs.
  dbStats: {
    tablesTitle: "Tables in {{database}}",
    collectionsTitle: "Collections in {{database}}",
    reload: "Reload statistics",
    loading: "Loading...",
    colTable: "Table",
    colCollection: "Collection",
    colRows: "Rows",
    colDocuments: "Documents",
    colDataSize: "Data size",
    colIndexSize: "Index size",
    colAvgRow: "Avg. row size",
    colAvgDocument: "Avg. document size",
    total: "Total",
    totalShown: "Total shown",
    bytes: "{{bytes}} bytes",
    searchTables: "Search tables...",
    searchCollections: "Search collections...",
    noTables: "This database has no tables.",
    noCollections: "This database has no collections.",
    noTablesMatch: "No table matches this search.",
    noCollectionsMatch: "No collection matches this search.",
    estimateNote:
      "Row counts and average row sizes are the estimates the server keeps, not exact counts. On an InnoDB table they can be well off.",
    sortNone: "Sort by {{column}}",
    sortAsc: "Sorted by {{column}}, smallest first",
    sortDesc: "Sorted by {{column}}, largest first",
  },
  // The rename dialog, wherever it is opened from: only the title says what is being renamed.
  renameDialog: {
    name: "New name",
    errorName: "The new name cannot be empty.",
    submit: "Rename",
    saving: "Renaming...",
  },
  // The collation picker, wherever one is declared — a column's or a whole table's.
  collation: {
    charsetDefault: "{{charset}} · default",
    // Shown beside MariaDB's `uca1400_*` collations, which name no character set of their own:
    // they stand for a whole family, and the server resolves one to the column's own character set
    // — `uca1400_ai_ci` on a utf8mb4 column becomes `utf8mb4_uca1400_ai_ci`.
    anyCharset: "any Unicode charset",
    search: "Search collations...",
  },
  // Creating a database, from either workspace's picker. Only MySQL's dialog has a collation in it;
  // the wording around the name is the same on both.
  databaseDialog: {
    title: "Create a database",
    name: "Name",
    collation: "Collation",
    collationPlaceholder: "(server default)",
    errorName: "The database needs a name.",
    submit: "Create database",
    saving: "Creating...",
  },
  tableDialog: {
    title: "Create a table in {{database}}",
    name: "Name",
    collation: "Collation",
    engine: "Engine",
    engineHint: "The engine cannot be changed after the table is created.",
    collationPlaceholder: "(database default)",
    columnHint:
      "The table is created with one column, id int(11) unsigned AUTO_INCREMENT, as its primary key. Add the rest from the Structure tab.",
    columnHintPostgres:
      "The table is created with one column, id integer GENERATED BY DEFAULT AS IDENTITY, as its primary key. Add the rest from the Structure tab. Name it schema.table to create it outside public.",
    columnHintClickhouse:
      "The table is created with one column, id UInt64, and an empty sorting key (ORDER BY tuple()). Add the rest from the Structure tab.",
    errorName: "The table needs a name.",
    submit: "Create table",
    saving: "Creating...",
  },
  columnDialog: {
    addTitle: "Add a column to {{table}}",
    editTitle: "Edit {{column}}",
    name: "Name",
    type: "Type",
    typePlaceholder: "Choose a type",
    typeArg: "Length or values",
    position: "Position",
    positionKeep: "Leave where it is",
    positionEnd: "At the end",
    positionFirst: "First",
    positionAfter: "After {{column}}",
    collation: "Collation",
    collationPlaceholder: "(table default)",
    comment: "Comment",
    nullable: "Allow NULL",
    unsigned: "UNSIGNED",
    autoIncrement: "AUTO_INCREMENT",
    onUpdate: "ON UPDATE CURRENT_TIMESTAMP",
    hasDefault: "Has a default value",
    defaultValue: "Default value",
    defaultIsExpression: "Read the default as an expression, not as text",
    defaultExpressionHint:
      "CURRENT_TIMESTAMP and NOW() are recognised on their own; tick this for any other expression, e.g. uuid().",
    identityLockedMssql:
      "SQL Server can't change an IDENTITY column's type, nullability or collation, only its name and comment. Drop and re-add the column to change the rest.",
    errorName: "The column needs a name.",
    errorType: "The column needs a type.",
    errorTypeArg: "{{type}} needs a list of values, e.g. 'a','b'.",
    errorTypeArgNumber: "The length must be a number, e.g. 255 or 10,2.",
    submitAdd: "Add column",
    submitEdit: "Save changes",
    saving: "Applying...",
  },
  indexDialog: {
    addTitle: "Add an index to {{table}}",
    editTitle: "Edit index {{index}}",
    name: "Name",
    namePlaceholder: "(named after its first column)",
    nameFixed: "A primary key is always called PRIMARY",
    kind: "Kind",
    kindIndex: "Index",
    kindUnique: "Unique",
    kindFulltext: "Full-text",
    kindSpatial: "Spatial",
    kindPrimary: "Primary key",
    method: "Method",
    methodDefault: "(engine default)",
    columns: "Columns",
    column: "Index column",
    addColumn: "Add a column",
    removeColumn: "Remove this column",
    prefixLength: "Prefix length",
    prefixPlaceholder: "whole",
    prefixTooltip: "Index only the first n characters. Required for a TEXT or BLOB column",
    comment: "Comment",
    replaceNote:
      "An index cannot be changed in place: this one is dropped and rebuilt in a single statement.",
    errorColumns: "An index needs at least one column.",
    errorDuplicateColumn: "{{column}} is named twice.",
    submitAdd: "Add index",
    submitEdit: "Save changes",
    saving: "Applying...",
  },
  skipIndexDialog: {
    addTitle: "Add a skip index to {{table}}",
    editTitle: "Edit skip index {{index}}",
    name: "Name",
    expr: "Expression",
    exprPlaceholder: "a column, or an expression like lower(note)",
    type: "Type",
    granularity: "Granularity",
    granularityHint:
      "Granularity counts granules (blocks of the table's own index_granularity rows each, usually 8192), not rows directly.",
    errorName: "The index has to be named.",
    errorExpr: "The index needs an expression to cover.",
    saving: "Saving…",
    submitAdd: "Add",
    submitEdit: "Save",
  },
  orderByDialog: {
    title: 'Change the sorting key of "{{table}}"',
    warning:
      "This copies the whole table into a temporary one built with the new key, then swaps names. It cannot be cancelled once started, and cannot be undone by this dialog.",
    warningWithCount:
      "This copies all {{count}} rows into a temporary table built with the new key, then swaps names. It cannot be cancelled once started, and cannot be undone by this dialog.",
    columns: "Sorting key columns",
    column: "Column",
    addColumn: "Add a column",
    removeColumn: "Remove this column",
    confirmLabel: 'Type "{{table}}" to confirm',
    errorColumns: "The sorting key needs at least one column.",
    errorDuplicateColumn: 'Column "{{column}}" is already in the sorting key.',
    saving: "Rebuilding…",
    submit: "Rebuild",
    leftoverWarning:
      'The sorting key was changed. Cleaning up the temporary table "{{table}}" failed. It holds the old data and is safe to drop by hand.',
  },
  // The Query tab. A script is run statement by statement, and each statement is reported as what
  // it is — a result set, a count of rows changed, or plain confirmation that it ran.
  query: {
    run: "Run",
    format: "Format",
    shortcutScope: "Query editor",
    shortcutFormat: "Format the script",
    running: "Running...",
    cancel: "Cancel",
    cancelling: "Cancelling...",
    // The shortcut itself is drawn as keycaps, so only the words around it live here.
    runShortcutHint: "runs the selection, or the whole script",
    selectionHint: "Select part of the script to run only that; with nothing selected the whole script runs.",
    editorHeading: "SQL",
    placeholder: "SELECT * FROM ...",
    editorLabel: "SQL editor",
    targetLabel: "Running on",
    // Short enough to stay on one line inside the chip; the rest is its tooltip.
    noDatabase: "No database selected",
    noDatabaseHint: "Write db.table in the script, or pick a database in the header.",
    resizeResults: "Drag to resize the results",
    // A script that ran fine and had nothing in it to run: all comments, or a stray semicolon.
    noStatements: "There was no statement to run.",
    noStatementsHint: "What was sent held only comments, so there was nothing for the server to do.",
    resultLabel: "#{{n}} {{verb}}",
    duration: "{{ms}} ms",
    rowCount: "{{n}} rows",
    truncated: "First {{n}} rows only. Add a LIMIT to be sure of what you are seeing",
    affected: "{{n}} rows changed",
    lastInsertId: "last inserted id {{id}}",
    ok: "OK",
    noRows: "The result set is empty.",
    statementFailed: "This statement failed, and nothing after it ran.",
    // The footer's actions, both about how much of the tab the results are given: put away to get
    // the window back for the script, or lifted out over the whole of it — which is how a run of
    // several SELECTs is read without each of them getting a quarter of a short pane.
    hideResults: "Hide the results",
    showResults: "Show the results",
    resultsEmpty: "Nothing has been run yet, so there are no results to hide.",
    // The two exclude each other: results lifted over the window cannot be put away from a bar
    // underneath them, and results put away cannot be lifted.
    resultsZoomed: "The results are covering the window. Close them first.",
    zoom: "Expand the results",
    zoomEmpty: "Nothing has been run yet, so there are no results to expand.",
    zoomShut: "The results are hidden. Show them first.",
    zoomTitle: "Results",
    // The chip in the toolbar, and what happens when a write is attempted anyway.
    readOnly: "Read-only",
    readOnlyBlocked: "Nothing was sent: this connection is marked read-only, and the script holds a {{verb}}.",
    // Shown instead of the two keys above when `dmlEvenIfReadOnly` is the reason: ClickHouse's
    // Query tab can send INSERT/UPDATE/DELETE/TRUNCATE even though its DDL/dump-restore stay
    // closed — see `SqlWorkspaceProps.dmlEvenIfReadOnly`.
    ddlOnlyReadOnly: "DDL locked",
    ddlOnlyReadOnlyHint:
      "INSERT/UPDATE/DELETE/TRUNCATE work here. Table and database changes still go through the Structure tab.",
    ddlBlocked:
      "Nothing was sent: this Query tab only takes INSERT/UPDATE/DELETE/TRUNCATE by hand. Other changes go through the Structure tab.",
    // The gate in front of an UPDATE, DELETE or TRUNCATE that names no rows.
    unguardedTitle: "Change every row?",
    unguardedOne: "{{verb}} on {{table}} says nothing about which rows, so it applies to all of them.",
    unguardedOneUnnamed: "This {{verb}} says nothing about which rows, so it applies to all of them.",
    unguardedMany: "{{n}} statements say nothing about which rows they apply to, so each applies to every row of its table: {{list}}.",
    // And in front of a DROP, or an ALTER that drops something. Rows come back from a backup; a
    // dropped table's triggers, grants and foreign keys do not, which is why it asks differently.
    unguardedDropTitle: "Remove it for good?",
    unguardedDrop: "{{verb}} on {{table}} removes it and everything in it. Nothing here can put it back.",
    unguardedDropUnnamed: "This {{verb}} removes what it names and everything in it. Nothing here can put it back.",
    unguardedManyMixed: "{{n}} statements each remove or rewrite the whole of what they name: {{list}}.",
    unguardedConfirm: "Run it",
    // Above the results when a script held more than one statement: how many of them ran, and what
    // the server spent on them altogether. A statement that fails stops the ones after it, and
    // without this line a script cut short reads exactly like a script that finished.
    scriptSummary: "{{n}} of {{m}} statements · {{ms}} ms",
    scriptSummaryAll: "{{m}} statements · {{ms}} ms",
    // The heading's tooltip, which says what one more click does — and says the thing that is
    // easiest to get wrong about it. Sorting here reorders the rows that came back and nothing
    // else: a result cut off at 1000 rows, sorted descending, does not show the table's largest.
    sortAsc: "Sort by {{column}}, smallest first (only the rows already returned)",
    sortDesc: "Sort by {{column}}, largest first (only the rows already returned)",
    sortNone: "Back to the order the server sent",
    findPlaceholder: "Filter these rows...",
    findCount: "{{n}} of {{m}} rows",
    // Not the same sentence as `noRows`: that one says the query found nothing, this one says the
    // box above cut everything out. Telling someone their query came back empty when it did not is
    // the kind of wrong answer they act on.
    noMatchingRows: "No row here matches that.",
    // The right-click menu over a result. Two halves: what is highlighted, and the whole result —
    // the second because the commonest reason to open this menu at all is to take the lot.
    copySelectionTsv: "Copy as TSV",
    copySelectionCsv: "Copy as CSV",
    copySelectionJson: "Copy as JSON",
    copyAllTsv: "Copy the whole result as TSV",
    copyAllCsv: "Copy the whole result as CSV",
    copyAllJson: "Copy the whole result as JSON",
    expandCell: "Open this cell",
    // Said when the script was sent with a ceiling it was not written with.
    limitAdded: "A LIMIT of {{limit}} was added to {{n}} of these statements. Write your own LIMIT to ask for something else.",
    // Saved queries, offered back by name as the editor is typed in.
    snippets: "Snippets",
    snippetHint: "Saved queries. Keep this one under a name, or fetch one back by typing its name.",
    snippetsTitle: "Saved queries",
    snippetNamePlaceholder: "Name this query...",
    saveSnippet: "Save",
    snippetSaving: "Saving...",
    snippetNothingToSave: "The editor is empty, so there is nothing to save. What you saved before is below.",
    snippetsEmpty: "Nothing saved yet. Name a query here and typing that name will offer it back.",
    snippetDelete: "Forget {{name}}",
    snippetDeleteConfirm: "Forget it?",
    snippetSaveFailed: "That could not be saved.",
    snippetDeleteFailed: "That could not be removed.",
    // Everything that has been run on this connection.
    history: "History",
    historyTitle: "Query history",
    historyFilter: "Search the queries...",
    historyClear: "Clear",
    // Only this connection's runs go, which is exactly what the list is showing.
    historyClearConfirm: "Clear this list?",
    // One run dropped rather than the lot, for the failed attempts a good query leaves behind it.
    historyDrop: "Forget this run",
    historyDropConfirm: "Forget it?",
    historyEmpty: "Nothing has been run on this connection yet.",
    historyNoMatch: "No query here contains that.",
    historyFailed: "failed",
    // The tooltip that appears when the pointer rests on a name in the script. The first line is
    // always the name itself, so these are only what is said around it.
    hoverTable: "table · {{n}} columns",
    hoverTableOne: "table · 1 column",
    hoverMoreColumns: "+{{n}} more",
    hoverJump: "{{shortcut}} opens it",
    hoverFunction: "MySQL function",
    hoverNullable: "nullable",
    hoverNotNull: "not null",
    hoverPrimaryKey: "primary key",
    hoverUniqueKey: "unique",
    hoverIndexed: "indexed",
  },
  // What the SQL editor underlines, and why. The first six are things the text says outright; the
  // last three are the editor comparing what was written against the database it can see, which is
  // why they are worded as observations rather than as verdicts.
  lint: {
    openString: "This quote is never closed.",
    openIdentifier: "This quoted name is never closed.",
    openComment: "This /* comment is never closed.",
    unclosedBracket: "This bracket is never closed.",
    strayBracket: "There is no opening bracket for this one.",
    danglingComma: "Nothing follows this comma.",
    unknownTable: "This database has no table called {{name}}.",
    unknownColumn: "{{table}} has no column called {{name}}.",
    unknownName: "No table in this statement has a column called {{name}}.",
    // The button on a squiggle's tooltip.
    replaceWith: "Use {{name}}",
  },
  // The condition bar above a grid. Everything here is the part that reads the same whichever
  // database is underneath — the operator labels themselves are per-workspace, under
  // `sqlTable.op.*` and `noSqlTable.op.*`.
  filterBar: {
    addFilter: "Add a filter",
    removeFilter: "Remove this filter",
    enableFilter: "Apply this filter",
    apply: "Apply",
    field: "Filter field",
    fieldSearch: "Search fields...",
    operator: "Filter operator",
    operatorSearch: "Search operators...",
    value: "Filter value",
    valuePlaceholder: "Value",
    listPlaceholder: "1,2,3",
    pairPlaceholder: "min,max",
  },
  sqlTable: {
    null: "NULL",
    loading: "Loading...",
    noRows: "No rows.",
    reloadRows: "Reload rows",
    // Named for the shortcut table in Settings; the chord itself is drawn from the registry.
    shortcutScope: "Table data",
    shortcutSelectAll: "Select every row on the page",
    shortcutFilter: "Jump to the filter bar",
    insertRows: "Add rows",
    cloneRows: "Clone selected rows",
    deleteRows: "Delete selected rows",
    deletingRows: "Deleting...",
    deleteRowsTitle: "Delete rows?",
    deleteRowsMessage: "Delete the {{n}} selected rows? This cannot be undone.",
    deleteAllRowsOption: "Delete all {{total}} rows in the table, not only the ones selected here",
    resetAutoIncrementOption: "Reset {{column}} so the next row inserted starts at 1",
    // `->` for the arrow, as everywhere else in the app. Shown in a `Tooltip` of the app's own
    // rather than through `title`, so it is drawn in the app's font like the grid around it.
    foreignKey: "Foreign key -> {{table}}.{{column}}",
    sortNone: "{{column}}: click to sort descending",
    sortDesc: "{{column}}: sorted descending, click to sort ascending",
    sortAsc: "{{column}}: sorted ascending, click to remove the sort",
    // The right-click menu over the rows. The row entries come in two spellings each, one for the
    // single row under the pointer and one for a selection — there is no plural rule in `t()`, and
    // "Copy 1 rows" is worse than a second key.
    copyCellValue: "Copy cell value",
    // The header has a menu of its own, holding this one entry: the column names cannot be
    // selected with the pointer, so this is the only way to take one.
    copyColumnName: "Copy column name",
    openReferencedRow: "Open the referenced row in {{table}}",
    copyInsert: "Copy as INSERT statement",
    copyInsertRows: "Copy {{n}} rows as INSERT statements",
    copyInsertWithout: "Copy as INSERT statement without {{column}}",
    copyInsertRowsWithout: "Copy {{n}} rows as INSERT statements without {{column}}",
    copyAsTsv: "Copy row as TSV",
    copyRowsAsTsv: "Copy {{n}} rows as TSV",
    copyAsCsv: "Copy row as CSV",
    copyRowsAsCsv: "Copy {{n}} rows as CSV",
    copyAsJson: "Copy row as JSON",
    copyRowsAsJson: "Copy {{n}} rows as JSON",
    expandCell: "Open this cell",
    // What the operator does in words, with the SQL it stands for in brackets — the four that
    // are all LIKE underneath are told apart by the shape of the pattern each one builds.
    op: {
      eq: "Equals (=)",
      ne: "Not equal (≠)",
      gt: "Greater than (>)",
      gte: "Greater or equal (≥)",
      lt: "Less than (<)",
      lte: "Less or equal (≤)",
      contains: "Contains (LIKE %a%)",
      notContains: "Not contains (NOT LIKE %a%)",
      startsWith: "Starts with (LIKE a%)",
      endsWith: "Ends with (LIKE %a)",
      like: "Matches pattern (LIKE)",
      notLike: "No pattern match (NOT LIKE)",
      regexp: "Matches regex (REGEXP)",
      notRegexp: "No regex match (NOT REGEXP)",
      in: "In list (IN)",
      notIn: "Not in list (NOT IN)",
      between: "In range (BETWEEN)",
      notBetween: "Out of range (NOT BETWEEN)",
      isNull: "Has no value (IS NULL)",
      isNotNull: "Has a value (IS NOT NULL)",
      isEmpty: "Empty string (= '')",
      isNotEmpty: "Non-empty string (≠ '')",
    },
  },
  insertRows: {
    title: "Add rows to {{table}}",
    cloneTitle: "Clone rows into {{table}}",
    addRow: "Add another row",
    removeRow: "Remove this row",
    rowNumber: "Row {{n}}",
    insert: "Insert {{n}} rows",
    inserting: "Inserting...",
    setNull: "Write NULL into this cell",
    unsetNull: "Stop writing NULL into this cell",
    notNullTooltip: "{{column}} is NOT NULL, so it cannot be left empty",
    notNullMarker: "NOT NULL",
    autoValue: "AUTO",
    autoTooltip: "The server assigns this value itself",
    generatedValue: "GENERATED",
    generatedTooltip: "A generated column. The server computes this value itself",
    defaultTooltip: "Left empty, this column falls back to its default: {{value}}",
    errorNotNull: "Row {{n}}: {{column}} is NOT NULL and cannot be set to NULL.",
    errorRequired: "Row {{n}}: {{column}} is NOT NULL, has no default, and needs a value.",
    transactionNote: "All rows are inserted together. If one fails, none are saved.",
  },
};

export type SqlDict = typeof sqlEn;

export default sqlEn;
