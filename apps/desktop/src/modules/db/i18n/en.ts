/**
 * What the database module calls things.
 *
 * Plain data, importing nothing from `src/i18n/`: `dicts.ts` imports this file, so anything
 * imported back out of there would close the circle. Groups stay flat at the top level, which is
 * what keeps every `t("connection.host")` in the module unchanged by the split.
 */
import sqlEn from "./sql/en";
import nosqlEn from "./nosql/en";

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
    // In the empty password box of a connection MixEngine handed over: the password is read from
    // MixEngine's keyring when Connect is pressed, and not before.
    passwordFromMixEngine: "Filled in from MixEngine when you connect",
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
    /** Shown where a workspace would be, for a kind `ConnectionWorkspace` has no workspace for: one this
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
  ...sqlEn,
  ...nosqlEn,
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
    notSqlConnection: "This is not a SQL connection.",
    unsupportedOperation: "This database does not support that action ({{operation}}).",
    tableEngineRequired: "Choose a table engine for the new table.",
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
    clickhouseReadOnly: "MixLab only reads from ClickHouse for now. Nothing here writes to it.",
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
