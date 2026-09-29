/**
 * What the database module calls things.
 *
 * Plain data, importing nothing from `src/i18n/`: `dicts.ts` imports this file, so anything
 * imported back out of there would close the circle. Groups stay flat at the top level, which is
 * what keeps every `t("connection.host")` in the module unchanged by the split.
 */
const dbEn = {
  connection: {
    selectPrivateKeyDialogTitle: "Select private key",
    selectSqliteFileDialogTitle: "Select SQLite database file",
    newSqliteFileDialogTitle: "New SQLite database file",
    allFilesFilter: "All files",
    testingTunnel: "Testing...",
    tunnelOk: "\u2713 Tunnel OK: SSH auth succeeded",
    tunnelFailed: "\u2717 {{error}}",
    connecting: "Connecting...",
    /* A `mixlab://` link opened from a browser: everything but the password came with it. */
    handoffNeedsPassword: "Enter the password to connect.",
    // On the button at the end of the workspace tabs. What it does is not "disconnect" alone —
    // the connection form comes back, still holding this connection, so it says where you land.
    disconnectHint: "Close this connection and go back to the connection form",
    connectedStatus: "Connected ({{id}})",
    fallbackTitle: "{{kind}} \u00b7 {{host}}",
    /** What Duplicate calls the copy it makes. */
    copySuffix: "{{name}} (copy)",
    changedElsewhere: "This connection was changed somewhere else. Your edits here stay until you load the new version.",
    loadNewVersion: "Load new version",
    removedElsewhere: "This connection was deleted somewhere else. Save connection keeps it as a new one.",
    nameLabel: "Name",
    saveAsLabel: "Save as",
    connectionNamePlaceholder: "Connection name",
    databaseLegend: "Database engine",
    kindMysql: "MySQL",
    kindPostgres: "PostgreSQL",
    kindMongo: "MongoDB",
    kindRedis: "Redis",
    kindSqlite: "SQLite",
    kindClickhouse: "ClickHouse",
    kindMssql: "SQL Server",
    /** Shown where a workspace would be, for a kind `DbTab` has no workspace for: one this
     *  build can connect to but not browse yet, or one saved by a newer version. */
    workspaceUnavailable:
      "Connected to {{kind}}. This build of MixLab cannot browse it yet, because there is no workspace for this database kind.",
    /** A `kind` this build doesn't list in `KIND_LABEL` — a connection saved by a newer version,
     *  read back by this one. See `kindLabel` in connectionForm.ts. */
    kindUnknown: "Unknown",
    sqlitePathLabel: "Database file",
    sqlitePathPlaceholder: "Path to a .db or .sqlite file",
    newSqliteFile: "New...",
    dbIndexLabel: "DB index",
    connectionStringLabel: "Connection string",
    connectionStringPlaceholder: "mongodb://user:password@host:27017/?authSource=admin&replicaSet=rs0",
    revealConnectionString: "Show and edit the connection string",
    hideConnectionString: "Hide the connection string",
    revealConnectionStringTitle: "Show the connection string?",
    revealConnectionStringMessage:
      "The connection string holds the username and password in plain text. They stay on screen until you hide them again.",
    revealConnectionStringConfirm: "Show",
    useSslLabel: "Use SSL",
    useSslHint: "Turn it off for a server with no SSL, or only a legacy one such as an old self-signed certificate.",
    // Shown while a Redis connection to another machine is left without a password. Protected mode
    // is Redis's default in exactly that case, and it hangs up on the connection rather than
    // answering — which reaches the user as "broken pipe" and explains nothing. An SSH tunnel only
    // helps when it lands on the machine Redis itself runs on, which is what the host says.
    redisNoPasswordWarning:
      "No password: if this server runs with protected mode on (Redis's default when the default user has no password), it only accepts connections coming from its own machine and closes every other one, which shows up as “Redis: broken pipe”. Set a password on the server (requirepass), or arrive from that machine: an SSH tunnel to the host Redis runs on, with the host above left at 127.0.0.1.",
    connectionMethodLegend: "Connection method",
    methodTcpIp: "Direct TCP/IP",
    methodSsh: "SSH tunnel",
    sshHost: "SSH host",
    sshPort: "SSH port",
    sshUser: "SSH user",
    auth: "Auth",
    authPassword: "Password",
    authPrivateKey: "Private key",
    sshPassword: "SSH password",
    privateKeyFile: "Private key file",
    keyPassphrase: "Key passphrase",
    passphrasePlaceholder: "(leave blank if none)",
    testTunnel: "Test tunnel",
    untitled: "New connection",
    notConnected: "Not connected",
    general: "General",
    changeEngine: "Change",
    engineAboutMysql: "The popular open-source SQL server, and MariaDB.",
    engineAboutPostgres: "An advanced open-source SQL server.",
    engineAboutSqlite: "A database in a single file on this computer.",
    engineAboutMongo: "A document database, reached by connection string.",
    engineAboutRedis: "An in-memory key-value store.",
    engineAboutClickhouse: "A column-oriented database for analytics.",
    engineAboutMssql: "Microsoft's SQL server.",
    server: "Server",
    file: "File",
    showPassword: "Show the password",
    hidePassword: "Hide the password",
    sqliteOpenedDirectly: "The file is opened directly on this computer, so there is nothing to route or tunnel.",
    authPasswordHint: "The SSH account's password",
    authPrivateKeyHint: "A key file, and its passphrase if it has one",
    route: "Route",
    thisComputer: "This computer",
    sshServer: "SSH server",
    routeDirect: "direct",
    routeEncrypted: "encrypted",
    routeLocal: "local",
    routeFile: "file access",
    copyConnectionString: "Copy the connection string",
    copied: "Copied",
    searchConnections: "Search connections",
    pinnedGroup: "Pinned",
    noConnectionsMatch: "No connection matches",
    clearFilters: "Clear filters",
    updateConnection: "Update connection",
    saveConnection: "Save connection",
    saveAsNew: "Save as new",
    connections: "Connections",
    newConnection: "New connection",
    savedItemTooltip: "Click to edit \u00b7 double-click to connect \u00b7 right-click for options",
    pin: "Pin to top",
    unpin: "Unpin",
    // Marks the connection as one MixLab will not send a write down. A reminder about which server
    // this is, not a permission — what the server allows is the credential's business.
    markReadOnly: "Mark read-only",
    allowWrites: "Allow writes",
    pinnedTooltip: "Pinned",
  },
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
  insertDocuments: {
    title: "Add documents to {{collection}}",
    cloneTitle: "Clone documents into {{collection}}",
    preparing: "Preparing ids...",
    addDocument: "Add another document",
    removeDocument: "Remove this document",
    serverAssignedId: "_id assigned by the server",
    insert: "Insert {{n}} documents",
    inserting: "Inserting...",
    idNote: "_id is filled in for you and can be edited; every other property is yours to add.",
    orderedNote:
      "Documents are inserted in order and not as one transaction. If one fails, the ones before it are already saved.",
    errorDuplicateId: "Documents {{a}} and {{b}} carry the same _id.",
  },
  mongo: {
    noCollections: "No collections",
    noMatchingCollections: "No matching collections",
    serverInfo: "{{os}} · MongoDB {{version}}",
    databaseLabel: "Database",
    databasePlaceholder: "(none)",
    searchDatabasesPlaceholder: "Search databases...",
    createDatabase: "Create a database",
    reloadDatabases: "Reload databases",
    createDatabaseHint:
      "MongoDB has no empty database: this one is kept here until you create its first collection, which is what stores it on the server.",
    databaseNameInvalid: "A database name cannot contain a space or any of / \\ . \" $ * < > : | ?",
    databaseExists: "There is already a database called {{database}}.",
    dataTab: "Data",
    statsTab: "Statistics",
    searchCollectionsPlaceholder: "Search collections...",
    reloadCollections: "Reload collections",
    addCollection: "Create a collection",
    addCollectionSystem: "{{database}} belongs to the server, so no collection can be added to it",
    systemCollection:
      "{{database}} belongs to the server, so its collections cannot be renamed or dropped",
    renameCollection: "Rename",
    dropCollection: "Drop collection",
    renameCollectionTitle: "Rename {{collection}}",
    dropCollectionTitle: "Drop collection?",
    dropCollectionMessage:
      "Drop {{collection}} and every document in it? This cannot be undone.",
    resizeSidebar: "Resize sidebar",
    resizeSidebarTooltip: "Drag to resize, double-click to fit",
    selectCollectionPrompt: "Select a collection to view its documents.",
    selectDatabaseStatsPrompt: "Select a database to see what its collections weigh.",
  },
  collectionDialog: {
    title: "Create a collection in {{database}}",
    name: "Name",
    errorName: "The collection needs a name.",
    submit: "Create collection",
    saving: "Creating...",
  },
  noSqlTable: {
    loading: "Loading...",
    saving: "Saving...",
    noDocuments: "No documents.",
    // What the operator does in words, with the Mongo query operator it stands for in brackets.
    // The four that are all $regex underneath are told apart by the pattern each one builds.
    op: {
      eq: "Equals ($eq)",
      ne: "Not equal ($ne)",
      gt: "Greater than ($gt)",
      gte: "Greater or equal ($gte)",
      lt: "Less than ($lt)",
      lte: "Less or equal ($lte)",
      contains: "Contains ($regex a)",
      notContains: "Not contains ($not /a/)",
      startsWith: "Starts with ($regex ^a)",
      endsWith: "Ends with ($regex a$)",
      regexp: "Matches regex ($regex)",
      notRegexp: "No regex match ($not)",
      in: "In list ($in)",
      notIn: "Not in list ($nin)",
      between: "In range ($gte + $lte)",
      notBetween: "Out of range ($lt or $gt)",
      type: "Is of type ($type: string, int, date...)",
      exists: "Field is present ($exists)",
      notExists: "Field is absent ($exists false)",
      isNull: "Is null ($type null)",
      isNotNull: "Has a non-null value",
      isEmpty: 'Empty string (= "")',
      isNotEmpty: 'Non-empty string (≠ "")',
    },
    reloadDocuments: "Reload documents",
    insertDocuments: "Add documents",
    cloneDocument: "Clone this document",
    idReadOnlyTooltip: "_id cannot be changed after a document exists",
    addProperty: "Add property",
    addItem: "Add item",
    propertyNamePlaceholder: "Property name",
    deleteProperty: "Delete property",
    undoDeleteProperty: "Undo delete",
    deleteItem: "Delete item",
    renameProperty: "Rename property",
    confirmRename: "Confirm rename",
    cancelRename: "Cancel rename",
    showMore: "+{{n}} more...",
    collapse: "Collapse",
    expand: "Expand",
    unsavedChanges: "{{n}} unsaved",
    saveError: "Failed to save",
    selectTypePlaceholder: "Type",
    maxDepthReached: "\u2026 (max depth reached)",
    discardChanges: "Discard changes",
    savedFlash: "Saved",
    deleteDocument: "Delete document",
    confirmDeleteDocument: "Delete this document? This cannot be undone.",
    collapseDocument: "Collapse document",
    expandDocument: "Expand document",
    typeLabel: {
      objectId: "ObjectId",
      string: "String",
      int32: "Int32",
      int64: "Int64",
      double: "Double",
      decimal128: "Decimal128",
      boolean: "Boolean",
      date: "Date",
      array: "Array",
      object: "Object",
      null: "Null",
      binary: "Binary",
      regExp: "Regular expression",
      timestamp: "Timestamp",
      minKey: "MinKey",
      maxKey: "MaxKey",
      javaScript: "JavaScript",
      javaScriptWithScope: "JavaScript with scope",
      symbol: "Symbol",
      undefined: "Undefined",
      dbPointer: "DBPointer",
    },
  },
  redis: {
    serverInfo: "{{os}} · Redis {{version}}",
    databaseLabel: "Database",
    dbOption: "db{{index}} ({{keys}} keys)",
    searchDatabasesPlaceholder: "Search databases...",
    reloadDatabases: "Reload databases",
    dataTab: "Keys",
    groupTab: "Delete keys",
    // Redis cannot drop a prefix in one call — the keyspace is flat and `user:*` is a pattern
    // over it — so removing a group means naming every key in it, which is what this lists.
    listGroupKeys: "List keys to delete",
    keyPatternPlaceholder: "Key pattern, e.g. user:*",
    keyPatternTooltip: "A Redis glob: * matches anything, ? one character, [ab] a set. Press Enter to scan.",
    // Redis keyspaces are flat; a separator is only a convention in how names are written, so
    // which character groups them is the user's to pick.
    separatorLabel: "Group keys by",
    separatorFlat: "No grouping",
    separatorFlatShort: "—",
    keyTreeLabel: "Keys",
    noKeys: "No keys",
    noKeysInSlice: "Nothing matched here yet. Load more to keep scanning.",
    reloadKeys: "Rescan keys",
    // The keyspace is walked with SCAN, which has no page numbers and no total to divide into
    // pages. The sidebar walks it to the end up front so the list can be sorted by name and stay
    // that way; these say how far that walk has got.
    scanningKeys: "Scanning... {{n}} keys",
    keysLoadedAll: "{{n}} keys",
    keysLoadedPartial: "{{n}} keys loaded, more in the keyspace",
    partialCountTooltip: "Keys read under this prefix so far. The scan is not finished, so there may be more.",
    scanLimitNotice: "Stopped early to keep the list readable. Raise the key limit, or narrow the key pattern, to see the rest in order.",
    // How far the scan is allowed to walk. Remembered per connection: the right number follows
    // the server, not the app.
    scanLimitLabel: "Key limit",
    scanLimitTooltip: "How many keys to read before the scan stops. The whole keyspace is read up front so the list can stay sorted by name, so a higher limit takes longer to open.",
    scanLimitShort: "{{n}}K",
    scanLimitOption: "Up to {{n}} keys",
    // Rows already in hand, held back only so the sidebar isn't laying out thousands at once.
    showMoreRows: "Show {{n}} more",
    loadMoreKeys: "Load more keys",
    loadingMore: "Loading...",
    resizeSidebar: "Resize sidebar",
    resizeSidebarTooltip: "Drag to resize, double-click to fit",
    selectKeyPrompt: "Select a key to view its value.",
  },
  redisValue: {
    loading: "Loading...",
    deleting: "Deleting...",
    noExpiry: "No expiry",
    expiresIn: "Expires in {{time}}",
    expired: "Key no longer exists",
    itemCount: "{{n}} items",
    field: "Field",
    score: "Score",
    member: "Value",
    value: "Value",
    emptyValue: "This key holds nothing.",
    keyGone: "This key no longer exists. It was deleted or has expired.",
    unsupportedType: "Values of type {{type}} cannot be shown here yet.",
    loadedOf: "{{loaded}} of {{total}} loaded",
    loadedCount: "{{loaded}} loaded",
    loadMoreItems: "Load more",
    // A stored value that parses as a JSON object or array — the string itself is all Redis
    // knows, so the formatted form is a reading of it and the raw form is what is really there.
    expandJson: "Show as formatted JSON",
    collapseJson: "Collapse back to one line",
    viewRaw: "Raw",
    viewFormatted: "JSON",
    reload: "Reload value",
    deleteKey: "Delete key",
    deleteKeyTitle: "Delete key?",
    deleteKeyMessage: "Delete {{key}} and everything it holds? This cannot be undone.",
  },
  redisGroup: {
    keyCount: "{{n}} keys under this prefix",
    // The sidebar's scan is what this list is made of, so a scan that stopped short lists less
    // than the prefix really holds — worth saying before anything is deleted from it.
    partialNotice:
      "The keyspace scan has not finished, so this is what has been read under the prefix rather than everything in it.",
    selectAll: "Select all",
    filterPlaceholder: "Filter these keys...",
    deleteSelected: "Delete selected ({{n}})",
    deleting: "Deleting... {{done}} of {{total}}",
    selectedCount: "{{selected}} of {{total}} selected",
    noKeys: "Nothing left under this prefix.",
    noMatches: "No keys match this filter.",
    confirmTitle: "Delete keys?",
    confirmMessage: "Delete {{n}} keys under {{prefix}}? This cannot be undone.",
  },
  // The command-line tools that do the dumping and restoring. MixLab does not ship them: it uses
  // whatever is on the machine, a copy it downloaded, or a path chosen here.
  tools: {
    title: "Dump tools",
    intro:
      "Dumping and restoring is done by the database vendors' own tools. MixLab uses whichever copy it can find, and can fetch one of its own.",
    mysqlSuite: "MySQL: mysqldump and mysql",
    postgresSuite: "PostgreSQL: pg_dump and psql",
    mongoSuite: "MongoDB: mongodump and mongorestore",
    download: "Download",
    redownload: "Download again",
    remove: "Delete copy",
    choose: "Choose...",
    forget: "Use the default",
    working: "Working...",
    // Tens of megabytes, so it has to say where it has got to — and say plainly when it is done.
    stageDownloading: "Downloading",
    stageVerifying: "Checking the download",
    stageUnpacking: "Unpacking",
    stageInstalling: "Putting it in place",
    sizeKnown: "{{done}} / {{total}} MB · {{percent}}%",
    sizeUnknown: "{{done}} MB",
    installed: "Downloaded and ready to use.",
    missing: "missing",
    missingHint: "Not found on this machine.",
    sourceCustom: "chosen",
    sourceDownloaded: "downloaded",
    sourceSystem: "installed",
    // Shown in place of the download button where the vendor publishes nothing for this machine.
    noDownload:
      "There is no download of these tools for this machine. Install them with your package manager (mysql-client or mariadb-client), or point MixLab at a copy below.",
    // EDB builds PostgreSQL binaries for Windows and macOS but stopped building Linux ones after
    // PostgreSQL 10, so this is what Linux is shown in place of the download button.
    noDownloadPostgres:
      "There is no download of the PostgreSQL client tools for this machine. Install postgresql-client with your package manager, or point MixLab at a copy below.",
  },
  // Dumping, restoring and dropping a whole database \u2014 the right-hand end of the sidebar's bar.
  dump: {
    dump: "Dump this database",
    restore: "Restore into this database",
    drop: "Drop this database",
    systemDatabase: "{{database}} belongs to the server, so it cannot be dumped, restored into or dropped",
    dumpTitle: "Dump {{database}}",
    modeAll: "Structure and data",
    modeAllHint: "Everything: the tables, what is in them, and the routines beside them.",
    modeStructure: "Structure only",
    modeStructureHint: "The tables, views, triggers and routines, with no rows.",
    modeData: "Data only",
    modeDataHint: "The rows alone, to be loaded into a database that already has the tables.",
    chooseFile: "Choose a file...",
    sqlFilter: "SQL dump",
    archiveFilter: "MongoDB archive",
    dumping: "Dumping {{database}}...",
    restoring: "Restoring into {{database}}...",
    dropping: "Dropping {{database}}...",
    installing: "Downloading the tools...",
    transferHint: "A large database can take several minutes. Leave MixLab open until it finishes.",
    cancelTransfer: "Stop",
    progressTables: "Table {{at}} of {{total}}",
    installTitle: "Download the tools?",
    installMysql:
      "Dumping needs mysqldump, which is not on this machine. MixLab can download the MySQL client tools from dev.mysql.com. The download is some tens to a couple of hundred megabytes, of which only a few files are kept. It happens once.",
    noDownload:
      "Dumping needs the MySQL client tools, which are not on this machine and are published for it only as a distribution package. Install mysql-client or mariadb-client with your package manager, or point MixLab at a copy in Settings.",
    installPostgres:
      "Dumping needs pg_dump, which is not on this machine. MixLab can download the PostgreSQL binaries from enterprisedb.com. The download is some hundreds of megabytes, of which only a few files are kept. It happens once.",
    noDownloadPostgres:
      "Dumping needs pg_dump and psql, which are not on this machine and have no download for it. Install postgresql-client with your package manager, or point MixLab at a copy in Settings.",
    installMongo:
      "Dumping needs mongodump, which is not on this machine. MixLab can download the MongoDB Database Tools from mongodb.com. It is about 60MB, downloaded once.",
    installConfirm: "Download",
    restoreTitle: "Restore into {{database}}?",
    restoreMysql:
      "Run {{file}} into {{database}}? Its tables replace the ones of the same name that are there now. A file carrying its own USE statement goes wherever that says instead. Nothing in MixLab can undo it.",
    restoreMongo:
      "Restore {{file}} into {{database}}? Its collections go there whatever database the archive was dumped from, and existing documents with the same _id are left as they are. Nothing in MixLab can undo it.",
    restoreConfirm: "Restore",
    dropTitle: "Drop database?",
    dropMysqlMessage:
      "Drop {{database}} with every table and row in it? This cannot be undone, so dump it first if you may want it back.",
    dropMongoMessage:
      "Drop {{database}} with every collection and document in it? This cannot be undone, so dump it first if you may want it back.",
  },
  tunnel: {
    reconnecting: "The SSH tunnel dropped. Opening it again\u2026",
    reconnected:
      "The tunnel is back. Anything the old connection held went with it: temporary tables, an open transaction, a script that was running.",
    failed: "The SSH tunnel could not be opened again: {{message}}",
    retry: "Try again",
    later: "Later",
  },
  // What a failed backend command says. The keys here are the `code` an `AppError` carries \u2014 see
  // src-tauri/src/error.rs \u2014 and `{{message}}` is where a driver's own words go, untranslated
  // because they are the server talking and the part worth searching for.
  error: {
    // The seven drivers, when what went wrong is what the server said.
    mysql: "MySQL: {{message}}",
    postgres: "PostgreSQL: {{message}}",
    mongo: "MongoDB: {{message}}",
    redis: "Redis: {{message}}",
    sqlite: "SQLite: {{message}}",
    clickhouse: "ClickHouse: {{message}}",
    mssql: "SQL Server: {{message}}",
    /** MySQL wanted the password in the clear and the link could carry it neither encrypted by
     *  TLS nor encrypted with the server's own key. See `connect_error` in
     *  `src-tauri/src/modules/db/drivers/mysql.rs` for why this should not happen any more. */
    mysqlSecureAuthRequired:
      "This MySQL server asked to check the password in full, and will only do that over a secure connection. Tick Use SSL and connect again.",
    /** A binary cell's text was not valid base64 — it did not come out of this app's own grid. */
    mssqlInvalidBinary: "This value is not valid base64 and cannot be written: {{message}}",
    /** SQL Server's string-to-money conversion silently drops a comma as a thousands separator
     *  instead of erroring — '9,9999' becomes 99999.0000, not 9.9999 with a mistyped separator. */
    mssqlAmbiguousMoney: "Use a period, not a comma, for the decimal point in a money value.",
    // Connections
    unknownConnection: "This connection is no longer open. Connect again.",
    wrongConnectionKind: "This is not a {{kind}} connection.",
    connectTimeout:
      "The {{kind}} connection timed out after {{seconds}}s. Check the host, the port and the firewall.",
    connectionLost:
      "The connection to the server was lost. If it goes through an SSH tunnel, MixLab is opening it again. Try once more in a moment.",
    noTunnel: "This connection does not go through an SSH tunnel.",
    mongoUriRequired: "A MongoDB connection string is required.",
    sqlitePathRequired: "Choose the SQLite database file to open.",
    /* MixLab never creates a database file: a path that is not there is a typo, not an empty
       database to start filling. */
    sqliteFileNotFound: "There is no file at {{path}}.",
    /* Refusing rather than replacing: nothing else in MixLab deletes a database file, and a New
       button is not where that should start. */
    sqliteFileExists: "There is already a file at {{path}}. Open it with Browse, or pick another name.",
    sqliteNoDatabases:
      "A SQLite database is a file. Creating or deleting one is done in the file manager, not here.",
    clickhouseReadOnly: "MixLab only reads from ClickHouse for now. Nothing here writes to it.",
    clickhouseOnlyFeature: "This only applies to a ClickHouse connection.",
    clickhouseMutationTimeout:
      "The mutation is still running on the server after 30 seconds. Reload the table to check whether it finished.",
    clickhouseMutationTargetUnknown:
      "Nothing to run this against: no database is selected, and the statement does not name one.",
    clickhouseHeterogeneousInsert:
      "These rows don't all fill in the same columns, so ClickHouse cannot insert them as one atomic statement.",
    clickhouseUnknownEngine:
      "MixLab does not create tables with the {{engine}} engine. Pick one of the MergeTree family.",
    clickhouseTypeChangeFailed:
      "Changing the type of {{column}} failed, and {{table}} cannot be read until it is put back: set the column's type to what it was before. The server said: {{cause}}",
    clickhouseSkipIndexExprRequired: "The skip index needs an expression to cover.",
    clickhouseOrderByColumnsRequired: "The sorting key needs at least one column.",
    clickhouseRebuildParse:
      "MixLab could not read this table's own definition back from the server, so the rebuild was not attempted.",
    clickhouseRebuildCountMismatch:
      "The row count changed while {{table}} was being copied (a write landed at the same time?). The rebuild was cancelled and the original table was not touched. Try again.",
    /* A `mixlab://connect?…` URL another program started MixLab with — see `handoff.ts`. The first
       is only ever printed to stderr; the second is answered with an empty form. Both exist so a
       code that does reach the screen one day is a sentence rather than its own key. */
    handoffInvalid: "The connection MixLab was started with cannot be read: {{message}}",
    handoffExpired: "This handed-over connection has already been opened.",
    mongoNoTcpHost: "The connection string names no TCP host to tunnel to.",
    emptyRedisCommand: "There is no command to run.",
    // Writing rows and documents
    updateWithoutKey: "This row has no column that identifies it, so it cannot be updated.",
    deleteWithoutKey: "This row has no column that identifies it, so it cannot be deleted.",
    rowsMatched: "Expected to match exactly 1 row, matched {{matched}}. Nothing was changed.",
    rowFailed: "Row {{index}}: {{cause}}",
    documentsMatched: "Expected to match exactly 1 document, matched {{matched}}.",
    documentsDeleted: "Expected to delete exactly 1 document, deleted {{deleted}}.",
    documentInvalid: "Document {{index}}: {{cause}}",
    documentNotObject: "Document {{index}} is not an object.",
    // Filters
    unknownFilterColumn: "The table has no column called {{column}}.",
    invalidFilterField: "{{field}} is not a field this filter can use.",
    unknownFilterOperator: "Unknown filter operator {{operator}}.",
    // Structure
    databaseNameRequired: "The database has to be named.",
    tableNameRequired: "The table has to be named.",
    collectionNameRequired: "The collection has to be named.",
    columnNameRequired: "The column has to be named.",
    columnTypeRequired: "The column needs a type.",
    indexNameRequired: "The index has to be named.",
    indexNeedsColumn: "An index needs at least one column.",
    indexColumnNameRequired: "Every column of an index has to be named.",
    invalidCollation: "{{collation}} is not a collation this server has.",
    unknownIndexKind: "Unknown index kind {{kind}}.",
    sqliteRenameWithOtherChanges:
      "Rename {{column}} on its own, then edit its type, default or collation separately.",
    sqliteNoPrimaryKeyAfterwards:
      "A SQLite primary key is part of the table itself and cannot be added to a table that has none.",
    sqliteIndexBelongsToConstraint:
      "{{index}} belongs to a PRIMARY KEY or UNIQUE constraint and cannot be dropped on its own.",
    sqliteColumnInTableConstraint:
      "{{column}} is part of a table-level constraint (a composite key, a UNIQUE, a CHECK or a FOREIGN KEY), and rewriting those is not supported yet.",
    sqliteColumnGenerated:
      "{{column}} is a generated column. Its value comes from its expression, not from an edit.",
    sqliteColumnIsPrimaryKey:
      "{{column}} is the table's own rowid, aliased as an INTEGER PRIMARY KEY, and cannot be changed.",
    sqliteRebuildForeignKeyViolation:
      "Rebuilding {{table}} would leave a foreign key pointing at a row that no longer matches. Nothing was changed.",
    unknownIndexType: "Unknown index type {{type}}.",
    mssqlRenameCannotChangeSchema:
      "SQL Server cannot move a table to another schema by renaming it. Rename it within {{schema}}, or drop and recreate it in the new one.",
    mssqlIdentityToggleNotSupported:
      "SQL Server can't turn IDENTITY on or off through ALTER COLUMN. Drop the column and add it again to change that.",
    noVisibleColumns:
      "No columns of {{database}}.{{table}} are visible. The table may not exist, or your user may have no privileges on it.",
    unknownColumn: "{{table}} no longer has a column called {{name}}.",
    nothingToRun: "There is nothing to run.",
    // BSON values
    bsonObjectId: "An ObjectId has to be a 24-character hex string.",
    bsonInt32Range: "That number is outside the range of an Int32.",
    bsonDate: "A date has to be written in RFC 3339, e.g. 2024-01-31T09:00:00.000Z.",
    bsonInvalidValue: "That is not a valid {{type}}.",
    bsonInvalidNumber: "That is not a number MixLab can store.",
    bsonMissingField: "A {{type}} needs its {{field}}.",
    bsonReadOnlyType: "{{type}} can be read but not written.",
    bsonUnknownType: "Unknown BSON type {{type}}.",
    // Dump and restore
    unknownDumpMode: "Unknown dump mode {{mode}}.",
    notMongoUri: "The connection string is not a mongodb:// URI.",
    srvOverTunnel:
      "Dumping over an SSH tunnel needs a plain mongodb:// connection string. A mongodb+srv:// one resolves its own hosts, which the tunnel does not reach.",
    notMongoArchive:
      "{{path}} is not a mongodump archive. MixLab restores the single-file archives its own dump writes.",
    archiveDatabaseUnreadable:
      "Cannot tell which database {{path}} holds. It may be compressed: {{message}}",
    archiveNamesNoDatabase: "{{path}} names no database to restore from.",
    noDumpAddress: "This connection has no address to dump from.",
    noDumpUri: "This connection has no connection string to dump with.",
    cannotRunTool: "Cannot run {{tool}}: {{message}}",
    toolWaitFailed: "{{tool}} could not be waited for: {{message}}",
    toolFailed: "{{tool}} failed:\n{{message}}",
    transferCancelled: "{{tool}} was stopped before it finished.",
    // The downloaded tools
    unknownTool: "Unknown tool {{tool}}.",
    unknownToolSuite: "Unknown tool suite {{suite}}.",
    mysqlToolNotFound:
      "{{tool}} was not found. Install the MySQL client tools, point MixLab at a copy in Settings, or let it download one.",
    // Said instead wherever MixLab has nothing to download, so that offering to is no help.
    mysqlToolNotInstalled:
      "{{tool}} was not found. Install the MySQL client tools with your package manager (mysql-client or mariadb-client), or point MixLab at a copy in Settings.",
    postgresToolNotFound:
      "{{tool}} was not found. Install the PostgreSQL client tools, point MixLab at a copy in Settings, or let it download one.",
    // Said instead wherever there is nothing to download, so that offering to is no help.
    postgresToolNotInstalled:
      "{{tool}} was not found. Install the PostgreSQL client tools (postgresql-client), or point MixLab at a copy in Settings.",
    mongoToolNotFound:
      "{{tool}} was not found. Install the MongoDB Database Tools, point MixLab at a copy in Settings, or let it download one.",
    noFileAt: "There is no file at {{path}}.",
    noMysqlArchive:
      "MySQL publishes no archive of its client tools for this platform. Install them through your package manager (mysql-client / mariadb-client) and MixLab will find them on PATH.",
    noPostgresArchive:
      "EnterpriseDB publishes no PostgreSQL binaries for this platform. Install the client tools through your package manager (postgresql-client) and MixLab will find them on PATH.",
    downloadFailed: "The download failed: {{message}}",
    unpackFailed: "Unpacking the download failed: {{message}}",
    downloadIncomplete:
      "The download did not contain the tools it was supposed to. The version MixLab asks for may have been withdrawn.",
    checksumMismatch:
      "The download is not the file MixLab expects (its checksum is {{actual}}, not {{expected}}). The release may have been withdrawn or replaced. Install the tools yourself and point MixLab at them in Settings.",
    cannotReadDownload: "Cannot read the download back: {{message}}",
    cannotCopyTool: "Cannot put {{tool}} in {{path}}: {{message}}",
    cannotSaveToolPath: "Cannot remember where that tool is: {{message}}",
    helperMissing: "{{program}} is needed for this and could not be run: {{message}}",
    // Files and the app's own directory
    cannotReadFile: "Cannot read {{path}}: {{message}}",
    cannotWriteFile: "Cannot write {{path}}: {{message}}",
    sqliteRestoreFailed: "The restore stopped at {{statement}}: {{message}}",
    mssqlRestoreFailed: "The restore stopped at {{statement}}: {{message}}",
    sqliteRebuildParseFailed:
      "MixLab could not read {{table}}'s own CREATE TABLE text well enough to rebuild it. It uses a syntax MixLab does not recognise yet.",
    cannotRemoveDirectory: "Cannot remove {{path}}: {{message}}",
    noAppDataDir: "There is nowhere for MixLab to keep its own files: {{message}}",
  },
  dbSync: {
    connections: "Connections",
    connectionSecrets: "Connection passwords",
    querySnippets: "Query snippets",
  },
};

export type DbDict = typeof dbEn;

export default dbEn;
