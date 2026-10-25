import type { DbDict } from "./en";
import sqlVi from "./sql/vi";
import nosqlVi from "./nosql/vi";

const dbVi: DbDict = {
  connection: {
    selectPrivateKeyDialogTitle: "Chọn private key",
    selectSqliteFileDialogTitle: "Chọn file database SQLite",
    newSqliteFileDialogTitle: "File database SQLite mới",
    allFilesFilter: "Tất cả file",
    testingTunnel: "Đang kiểm tra...",
    tunnelOk: "\u2713 Tunnel OK: xác thực SSH thành công",
    tunnelFailed: "\u2717 {{error}}",
    connecting: "Đang kết nối...",
    handoffNeedsPassword: "Nhập mật khẩu để kết nối.",
    passwordFromMixEngine: "MixEngine điền vào khi kết nối",
    disconnectHint: "Đóng kết nối này và quay lại màn hình kết nối",
    connectedStatus: "Đã kết nối ({{id}})",
    fallbackTitle: "{{kind}} \u00b7 {{host}}",
    copySuffix: "{{name}} (bản sao)",
    changedElsewhere: "Kết nối này vừa được sửa ở nơi khác. Phần bạn đang sửa ở đây vẫn giữ nguyên cho tới khi bạn tải bản mới.",
    loadNewVersion: "Tải bản mới",
    removedElsewhere: "Kết nối này đã bị xoá ở nơi khác. Bấm Lưu kết nối để giữ nó thành một kết nối mới.",
    nameLabel: "Tên",
    saveAsLabel: "Lưu thành",
    connectionNamePlaceholder: "Tên kết nối",
    databaseLegend: "Hệ quản trị",
    kindMysql: "MySQL",
    kindPostgres: "PostgreSQL",
    kindMongo: "MongoDB",
    kindRedis: "Redis",
    kindSqlite: "SQLite",
    kindClickhouse: "ClickHouse",
    kindMssql: "SQL Server",
    workspaceUnavailable:
      "Đã kết nối {{kind}}. Bản MixLab này chưa duyệt được vì chưa có workspace cho loại cơ sở dữ liệu này.",
    kindUnknown: "Không rõ",
    sqlitePathLabel: "File database",
    sqlitePathPlaceholder: "Đường dẫn tới file .db hoặc .sqlite",
    newSqliteFile: "Tạo mới...",
    dbIndexLabel: "Chỉ mục DB",
    connectionStringLabel: "Chuỗi kết nối",
    connectionStringPlaceholder: "mongodb://user:password@host:27017/?authSource=admin&replicaSet=rs0",
    revealConnectionString: "Hiện và sửa chuỗi kết nối",
    hideConnectionString: "Ẩn chuỗi kết nối",
    revealConnectionStringTitle: "Hiện chuỗi kết nối?",
    revealConnectionStringMessage:
      "Chuỗi kết nối chứa tên đăng nhập và mật khẩu dạng văn bản thuần. Chúng sẽ hiển thị trên màn hình cho đến khi bạn ẩn lại.",
    revealConnectionStringConfirm: "Hiện",
    useSslLabel: "Dùng SSL",
    useSslHint: "Tắt đi nếu server không có SSL, hoặc chỉ có SSL cũ như chứng chỉ tự ký cũ.",
    // Shown when a Redis connection to another machine has an empty password. In exactly that case
    // Redis turns on protected mode by default and cuts the connection instead of answering — the
    // user only sees "broken pipe". SSH only helps when the tunnel goes to the very machine running
    // Redis, i.e. the host above.
    redisNoPasswordWarning:
      "Chưa nhập mật khẩu: nếu server bật protected mode (mặc định của Redis khi người dùng default chưa có mật khẩu), nó chỉ nhận kết nối đến từ chính máy nó và cắt mọi kết nối khác, hiện ra thành “Redis: broken pipe”. Đặt mật khẩu trên server (requirepass), hoặc đi vào từ chính máy đó: mở SSH tới máy đang chạy Redis và để host ở trên là 127.0.0.1.",
    connectionMethodLegend: "Phương thức kết nối",
    methodTcpIp: "TCP/IP trực tiếp",
    methodSsh: "SSH tunnel",
    sshHost: "SSH host",
    sshPort: "SSH port",
    sshUser: "Người dùng SSH",
    auth: "Xác thực",
    authPassword: "Mật khẩu",
    authPrivateKey: "Private key",
    sshPassword: "Mật khẩu SSH",
    privateKeyFile: "File private key",
    keyPassphrase: "Passphrase của key",
    passphrasePlaceholder: "(để trống nếu không có)",
    testTunnel: "Kiểm tra tunnel",
    untitled: "Kết nối mới",
    notConnected: "Chưa kết nối",
    general: "Chung",
    changeEngine: "Đổi",
    engineAboutMysql: "SQL server mã nguồn mở phổ biến, và MariaDB.",
    engineAboutPostgres: "SQL server mã nguồn mở nâng cao.",
    engineAboutSqlite: "Một database gói trong một file trên máy này.",
    engineAboutMongo: "Database dạng document, kết nối bằng connection string.",
    engineAboutRedis: "Kho key-value trong bộ nhớ.",
    engineAboutClickhouse: "Database dạng cột cho phân tích.",
    engineAboutMssql: "SQL server của Microsoft.",
    server: "Server",
    file: "File",
    showPassword: "Hiện mật khẩu",
    hidePassword: "Ẩn mật khẩu",
    sqliteOpenedDirectly: "File được mở thẳng trên máy này, nên không cần định tuyến hay tunnel.",
    authPasswordHint: "Mật khẩu của tài khoản SSH",
    authPrivateKeyHint: "Một file key, và passphrase nếu có",
    route: "Đường đi",
    thisComputer: "Máy này",
    sshServer: "SSH server",
    routeDirect: "trực tiếp",
    routeEncrypted: "mã hoá",
    routeLocal: "nội bộ",
    routeFile: "truy cập file",
    copyConnectionString: "Chép connection string",
    copied: "Đã chép",
    searchConnections: "Tìm kết nối",
    pinnedGroup: "Đã ghim",
    noConnectionsMatch: "Không có kết nối nào khớp",
    clearFilters: "Bỏ bộ lọc",
    updateConnection: "Cập nhật kết nối",
    saveConnection: "Lưu kết nối",
    saveAsNew: "Lưu thành mới",
    connections: "Kết nối",
    newConnection: "Kết nối mới",
    pin: "Ghim lên đầu",
    unpin: "Bỏ ghim",
    // Marks the connection as one MixLab will not send write statements to. It is a reminder of
    // which server you are on, not a permission — what the server allows is up to the login
    // account.
    markReadOnly: "Đánh dấu chỉ đọc",
    allowWrites: "Cho phép ghi",
    pinnedTooltip: "Đã ghim",
    savedItemTooltip: "Bấm để sửa \u00b7 bấm đúp để kết nối \u00b7 chuột phải để xem tuỳ chọn",
  },
  ...sqlVi,
  ...nosqlVi,
  // The command-line tools that handle dump/restore. MixLab does not bundle them: it uses tools
  // already on the machine, a version it downloads itself, or a path you choose here.
  tools: {
    title: "Công cụ dump",
    intro:
      "Việc dump và restore do chính công cụ của MySQL/PostgreSQL/MongoDB đảm nhiệm. MixLab dùng bản nào tìm được, và có thể tự tải một bản riêng.",
    mysqlSuite: "MySQL: mysqldump và mysql",
    postgresSuite: "PostgreSQL: pg_dump và psql",
    mongoSuite: "MongoDB: mongodump và mongorestore",
    download: "Tải về",
    redownload: "Tải lại",
    remove: "Xoá bản đã tải",
    choose: "Chọn file...",
    forget: "Dùng mặc định",
    working: "Đang xử lý...",
    // The download weighs a few dozen MB, so it has to say how far along it is, and say so plainly
    // when done.
    stageDownloading: "Đang tải về",
    stageVerifying: "Đang kiểm tra bản tải",
    stageUnpacking: "Đang giải nén",
    stageInstalling: "Đang cài vào máy",
    sizeKnown: "{{done}} / {{total}} MB · {{percent}}%",
    sizeUnknown: "{{done}} MB",
    installed: "Đã tải xong, dùng được ngay.",
    missing: "chưa có",
    missingHint: "Không tìm thấy trên máy này.",
    sourceCustom: "tự chọn",
    sourceDownloaded: "đã tải",
    sourceSystem: "có sẵn",
    // Shown instead of the download button, on machines for which the vendor publishes no
    // downloadable build.
    noDownload:
      "Không có bản tải nào cho máy này. Cài bằng trình quản lý gói (mysql-client hoặc mariadb-client), hoặc chỉ cho MixLab một bản có sẵn ở dưới.",
    // EDB builds binaries for Windows and macOS, but stopped building for Linux after
    // PostgreSQL 10, so this is the sentence Linux sees instead of the download button.
    noDownloadPostgres:
      "Không có bản tải PostgreSQL client tools nào cho máy này. Cài postgresql-client bằng trình quản lý gói, hoặc chỉ cho MixLab một bản có sẵn ở dưới.",
  },
  // Dump, restore and dropping a whole database — the button group on the right of the sidebar's
  // action bar.
  dump: {
    dump: "Dump cơ sở dữ liệu này",
    restore: "Restore vào cơ sở dữ liệu này",
    drop: "Xoá cơ sở dữ liệu này",
    systemDatabase: "{{database}} là cơ sở dữ liệu hệ thống, không dump, restore hay xoá được",
    dumpTitle: "Dump {{database}}",
    modeAll: "Cả cấu trúc và dữ liệu",
    modeAllHint: "Toàn bộ: bảng, dữ liệu bên trong, và các routine đi kèm.",
    modeStructure: "Chỉ cấu trúc",
    modeStructureHint: "Bảng, view, trigger và routine, không kèm dòng dữ liệu nào.",
    modeData: "Chỉ dữ liệu",
    modeDataHint: "Chỉ các dòng dữ liệu, để nạp vào một cơ sở dữ liệu đã có sẵn bảng.",
    chooseFile: "Chọn nơi lưu...",
    sqlFilter: "File SQL",
    archiveFilter: "Archive MongoDB",
    dumping: "Đang dump {{database}}...",
    restoring: "Đang restore vào {{database}}...",
    dropping: "Đang xoá {{database}}...",
    installing: "Đang tải công cụ...",
    transferHint: "Cơ sở dữ liệu lớn có thể mất vài phút. Giữ MixLab mở cho tới khi xong.",
    cancelTransfer: "Dừng",
    progressTables: "Bảng {{at}}/{{total}}",
    installTitle: "Tải công cụ về?",
    installMysql:
      "Dump cần mysqldump, máy này chưa có. MixLab có thể tải bộ MySQL client tools từ dev.mysql.com. Bản tải nặng từ vài chục tới vài trăm MB, nhưng chỉ giữ lại vài file cần thiết. Chỉ tải một lần.",
    noDownload:
      "Dump cần bộ MySQL client tools, máy này chưa có mà nhà cung cấp chỉ phát hành dạng gói cài đặt cho nền tảng này. Cài mysql-client hoặc mariadb-client bằng trình quản lý gói, hoặc chỉ cho MixLab một bản có sẵn trong phần Cài đặt.",
    installPostgres:
      "Dump cần pg_dump, máy này chưa có. MixLab có thể tải bộ PostgreSQL binaries từ enterprisedb.com. Bản tải nặng vài trăm MB, nhưng chỉ giữ lại vài file cần thiết. Chỉ tải một lần.",
    noDownloadPostgres:
      "Dump cần pg_dump và psql, máy này chưa có mà cũng không có bản tải nào cho nền tảng này. Cài postgresql-client bằng trình quản lý gói, hoặc chỉ cho MixLab một bản có sẵn trong phần Cài đặt.",
    installMongo:
      "Dump cần mongodump, máy này chưa có. MixLab có thể tải bộ MongoDB Database Tools từ mongodb.com, khoảng 60MB, chỉ tải một lần.",
    installConfirm: "Tải về",
    restoreTitle: "Restore vào {{database}}?",
    restoreMysql:
      "Nạp {{file}} vào {{database}}? Các bảng trong file sẽ ghi đè bảng trùng tên đang có. Riêng file nào tự chứa lệnh USE thì nó vào database ghi trong đó. MixLab không hoàn tác được.",
    restoreMongo:
      "Restore {{file}} vào {{database}}? Các collection sẽ vào đây bất kể archive được dump từ cơ sở dữ liệu nào, và document trùng _id sẽ được giữ nguyên. MixLab không hoàn tác được.",
    restoreConfirm: "Restore",
    dropTitle: "Xoá cơ sở dữ liệu?",
    dropMysqlMessage:
      "Xoá {{database}} cùng toàn bộ bảng và dữ liệu trong đó? Thao tác này không hoàn tác được, nên dump trước nếu còn cần.",
    dropMongoMessage:
      "Xoá {{database}} cùng toàn bộ collection và document trong đó? Thao tác này không hoàn tác được, nên dump trước nếu còn cần.",
  },
  // Messages when a backend command fails. The key here is exactly the `code` `AppError` carries
  // — see src-tauri/src/error.rs. `{{message}}` is the driver's verbatim text, not translated: it
  // is
  tunnel: {
    reconnecting: "Mất kết nối SSH. Đang kết nối lại…",
    reconnected:
      "Đã kết nối lại. Những gì thuộc về kết nối cũ đã mất theo nó: bảng tạm, transaction đang mở, script đang chạy dở.",
    failed: "Không mở lại được tunnel SSH: {{message}}",
    retry: "Thử lại",
    later: "Để sau",
  },
  // the server speaking, and also the part most worth looking up.
  error: {
    // The six driver kinds, when the error comes from the server itself.
    mysql: "MySQL: {{message}}",
    postgres: "PostgreSQL: {{message}}",
    mongo: "MongoDB: {{message}}",
    redis: "Redis: {{message}}",
    sqlite: "SQLite: {{message}}",
    clickhouse: "ClickHouse: {{message}}",
    mssql: "SQL Server: {{message}}",
    mysqlSecureAuthRequired:
      "Máy chủ MySQL này yêu cầu kiểm tra mật khẩu đầy đủ, và chỉ chấp nhận điều đó trên kết nối đã mã hoá. Bật Use SSL rồi kết nối lại.",
    mssqlInvalidBinary: "Giá trị này không phải base64 hợp lệ, không ghi được: {{message}}",
    mssqlAmbiguousMoney: "Dùng dấu chấm, không dùng dấu phẩy, cho phần thập phân của giá trị tiền tệ.",
    // Connection
    unknownConnection: "Kết nối này không còn mở. Kết nối lại để tiếp tục.",
    wrongConnectionKind: "Đây không phải kết nối {{kind}}.",
    notSqlConnection: "Đây không phải kết nối SQL.",
    unsupportedOperation: "Cơ sở dữ liệu này không hỗ trợ thao tác đó ({{operation}}).",
    tableEngineRequired: "Chọn engine cho bảng mới.",
    connectTimeout:
      "Kết nối {{kind}} quá hạn sau {{seconds}} giây. Kiểm tra host, cổng và tường lửa.",
    connectionLost:
      "Mất kết nối tới máy chủ. Nếu kết nối này đi qua SSH tunnel, MixLab đang mở lại. Thử lại sau giây lát.",
    noTunnel: "Kết nối này không đi qua SSH tunnel.",
    mongoUriRequired: "Cần có chuỗi kết nối MongoDB.",
    sqlitePathRequired: "Chọn file database SQLite cần mở.",
    sqliteFileNotFound: "Không có file nào ở {{path}}.",
    sqliteFileExists: "Đã có file ở {{path}}. Dùng Duyệt để mở nó, hoặc chọn tên khác.",
    clickhouseReadOnly: "MixLab hiện chỉ đọc ClickHouse, chưa ghi được gì vào đó.",
    clickhouseMutationTimeout:
      "Thao tác vẫn đang chạy trên server sau 30 giây. Tải lại bảng để xem đã xong chưa.",
    clickhouseMutationTargetUnknown:
      "Không có gì để chạy lệnh này: chưa chọn database, và câu lệnh cũng không nêu tên.",
    clickhouseHeterogeneousInsert:
      "Các dòng này không khai báo cùng một tập cột, nên không thể chèn chung thành một câu lệnh nguyên tử trên ClickHouse.",
    clickhouseUnknownEngine:
      "MixLab không tạo bảng với engine {{engine}}. Chọn một engine thuộc họ MergeTree.",
    clickhouseTypeChangeFailed:
      "Đổi kiểu cột {{column}} thất bại, và bảng {{table}} không đọc được cho tới khi kiểu cũ được đặt lại: hãy sửa kiểu của cột về như trước. Server báo: {{cause}}",
    clickhouseSkipIndexExprRequired: "Skip index cần một biểu thức để lọc theo.",
    clickhouseOrderByColumnsRequired: "Sorting key cần ít nhất một cột.",
    clickhouseRebuildParse:
      "MixLab không đọc lại được định nghĩa của bảng này từ server nên chưa thử rebuild.",
    clickhouseRebuildCountMismatch:
      "Số dòng bị lệch trong lúc copy bảng {{table}} (có ghi đồng thời?). Đã huỷ rebuild, bảng gốc không hề bị đụng. Thử lại.",
    handoffInvalid: "Không đọc được kết nối MixLab được mở cùng: {{message}}",
    handoffExpired: "Kết nối được chuyển sang này đã được mở rồi.",
    mongoNoTcpHost: "Chuỗi kết nối không chỉ ra host TCP nào để mở tunnel.",
    emptyRedisCommand: "Không có lệnh nào để chạy.",
    // Writing rows and documents
    updateWithoutKey: "Dòng này không có cột nào định danh nó, nên không thể cập nhật.",
    deleteWithoutKey: "Dòng này không có cột nào định danh nó, nên không thể xoá.",
    rowsMatched: "Lẽ ra phải khớp đúng 1 dòng, nhưng khớp {{matched}} dòng. Không thay đổi gì.",
    rowFailed: "Dòng {{index}}: {{cause}}",
    documentsMatched: "Lẽ ra phải khớp đúng 1 document, nhưng khớp {{matched}}.",
    documentsDeleted: "Lẽ ra phải xoá đúng 1 document, nhưng đã xoá {{deleted}}.",
    documentInvalid: "Document {{index}}: {{cause}}",
    documentNotObject: "Document {{index}} không phải là một object.",
    // Filters
    unknownFilterColumn: "Bảng không có cột nào tên {{column}}.",
    invalidFilterField: "{{field}} không phải trường mà bộ lọc này dùng được.",
    unknownFilterOperator: "Toán tử lọc {{operator}} không hợp lệ.",
    // Structure
    databaseNameRequired: "Cơ sở dữ liệu phải có tên.",
    tableNameRequired: "Bảng phải có tên.",
    collectionNameRequired: "Collection phải có tên.",
    columnNameRequired: "Cột phải có tên.",
    columnTypeRequired: "Cột phải có kiểu dữ liệu.",
    indexNameRequired: "Index phải có tên.",
    indexNeedsColumn: "Index cần ít nhất một cột.",
    indexColumnNameRequired: "Mọi cột trong index đều phải có tên.",
    invalidCollation: "Máy chủ này không có collation {{collation}}.",
    unknownIndexKind: "Loại index {{kind}} không hợp lệ.",
    sqliteRenameWithOtherChanges:
      "Đổi tên {{column}} riêng một lần, rồi sửa kiểu/default/collation ở một lần khác.",
    sqliteNoPrimaryKeyAfterwards:
      "Primary key của SQLite là một phần của chính bảng, không thêm được vào bảng đã tạo mà chưa có.",
    sqliteIndexBelongsToConstraint:
      "{{index}} thuộc về một ràng buộc PRIMARY KEY hoặc UNIQUE, không xoá riêng được.",
    sqliteColumnInTableConstraint:
      "{{column}} là một phần của ràng buộc cấp bảng (khoá ghép, UNIQUE, CHECK hay FOREIGN KEY), và việc viết lại những thứ đó chưa được hỗ trợ.",
    sqliteColumnGenerated:
      "{{column}} là cột generated. Giá trị của nó đến từ biểu thức, không sửa tay được.",
    sqliteColumnIsPrimaryKey:
      "{{column}} chính là rowid của bảng, được đặt bí danh INTEGER PRIMARY KEY, không đổi được.",
    sqliteRebuildForeignKeyViolation:
      "Dựng lại {{table}} sẽ để lại một khoá ngoại trỏ vào một dòng không còn khớp. Chưa có gì bị đổi.",
    unknownIndexType: "Kiểu index {{type}} không hợp lệ.",
    mssqlRenameCannotChangeSchema:
      "SQL Server không thể chuyển bảng sang schema khác bằng cách đổi tên. Đổi tên trong {{schema}}, hoặc xoá và tạo lại ở schema mới.",
    mssqlIdentityToggleNotSupported:
      "SQL Server không thể bật/tắt IDENTITY qua ALTER COLUMN. Xoá cột rồi thêm lại nếu cần đổi.",
    noVisibleColumns:
      "Không thấy cột nào của {{database}}.{{table}}. Có thể bảng không tồn tại, hoặc tài khoản của bạn không có quyền trên bảng đó.",
    unknownColumn: "{{table}} không còn cột nào tên {{name}}.",
    nothingToRun: "Không có gì để chạy.",
    // BSON values
    bsonObjectId: "ObjectId phải là chuỗi hex 24 ký tự.",
    bsonInt32Range: "Số này nằm ngoài phạm vi của Int32.",
    bsonDate: "Ngày giờ phải viết theo RFC 3339, ví dụ 2024-01-31T09:00:00.000Z.",
    bsonInvalidValue: "Đây không phải giá trị {{type}} hợp lệ.",
    bsonInvalidNumber: "Đây không phải số mà MixLab lưu được.",
    bsonMissingField: "Giá trị {{type}} cần có {{field}}.",
    bsonReadOnlyType: "{{type}} chỉ đọc được, không ghi được.",
    bsonUnknownType: "Kiểu BSON {{type}} không hợp lệ.",
    // Dump and restore
    unknownDumpMode: "Chế độ dump {{mode}} không hợp lệ.",
    notMongoUri: "Chuỗi kết nối không phải URI mongodb://.",
    srvOverTunnel:
      "Dump qua SSH tunnel cần chuỗi kết nối mongodb:// thường. Chuỗi mongodb+srv:// tự phân giải host của nó, mà tunnel không tới được những host đó.",
    notMongoArchive:
      "{{path}} không phải archive của mongodump. MixLab chỉ restore được archive một file do chính nó dump ra.",
    archiveDatabaseUnreadable:
      "Không xác định được {{path}} chứa cơ sở dữ liệu nào. Có thể file đã bị nén: {{message}}",
    archiveNamesNoDatabase: "{{path}} không ghi cơ sở dữ liệu nào để restore.",
    noDumpAddress: "Kết nối này không có địa chỉ để dump.",
    noDumpUri: "Kết nối này không có chuỗi kết nối để dump.",
    cannotRunTool: "Không chạy được {{tool}}: {{message}}",
    toolWaitFailed: "Không chờ được {{tool}} chạy xong: {{message}}",
    toolFailed: "{{tool}} thất bại:\n{{message}}",
    transferCancelled: "{{tool}} đã bị dừng trước khi chạy xong.",
    // Downloaded tool suites
    unknownTool: "Công cụ {{tool}} không hợp lệ.",
    unknownToolSuite: "Bộ công cụ {{suite}} không hợp lệ.",
    mysqlToolNotFound:
      "Không tìm thấy {{tool}}. Cài MySQL client tools, trỏ MixLab tới một bản có sẵn trong Cài đặt, hoặc để MixLab tự tải về.",
    // Used instead where MixLab has nothing to download, so inviting a download is pointless.
    mysqlToolNotInstalled:
      "Không tìm thấy {{tool}}. Cài MySQL client tools bằng trình quản lý gói (mysql-client hoặc mariadb-client), hoặc trỏ MixLab tới một bản có sẵn trong Cài đặt.",
    postgresToolNotFound:
      "Không tìm thấy {{tool}}. Cài PostgreSQL client tools, trỏ MixLab tới một bản có sẵn trong Cài đặt, hoặc để MixLab tự tải về.",
    // Used instead where there is nothing to download, so inviting a download is pointless.
    postgresToolNotInstalled:
      "Không tìm thấy {{tool}}. Cài PostgreSQL client tools (postgresql-client), hoặc trỏ MixLab tới một bản có sẵn trong Cài đặt.",
    mongoToolNotFound:
      "Không tìm thấy {{tool}}. Cài MongoDB Database Tools, trỏ MixLab tới một bản có sẵn trong Cài đặt, hoặc để MixLab tự tải về.",
    noFileAt: "Không có file nào ở {{path}}.",
    noMysqlArchive:
      "MySQL không phát hành bản nén nào của client tools cho nền tảng này. Cài qua trình quản lý gói (mysql-client / mariadb-client), MixLab sẽ tự tìm thấy trên PATH.",
    noPostgresArchive:
      "EnterpriseDB không phát hành bản PostgreSQL binaries nào cho nền tảng này. Cài client tools qua trình quản lý gói (postgresql-client), MixLab sẽ tự tìm thấy trên PATH.",
    downloadFailed: "Tải về thất bại: {{message}}",
    unpackFailed: "Giải nén bản tải về thất bại: {{message}}",
    downloadIncomplete:
      "Bản tải về không chứa các công cụ cần có. Có thể phiên bản MixLab yêu cầu đã bị gỡ khỏi máy chủ.",
    checksumMismatch:
      "Bản tải về không đúng file MixLab mong đợi (checksum là {{actual}}, không phải {{expected}}). Có thể bản phát hành đã bị gỡ hoặc thay thế. Tự cài công cụ rồi trỏ MixLab tới chúng trong Cài đặt.",
    cannotReadDownload: "Không đọc lại được bản tải về: {{message}}",
    cannotCopyTool: "Không đặt được {{tool}} vào {{path}}: {{message}}",
    cannotSaveToolPath: "Không ghi nhớ được vị trí công cụ đó: {{message}}",
    helperMissing: "Thao tác này cần {{program}} nhưng không chạy được: {{message}}",
    // The application's own files and directories
    cannotReadFile: "Không đọc được {{path}}: {{message}}",
    cannotWriteFile: "Không ghi được {{path}}: {{message}}",
    sqliteRestoreFailed: "Khôi phục dừng lại ở {{statement}}: {{message}}",
    mssqlRestoreFailed: "Khôi phục dừng lại ở {{statement}}: {{message}}",
    sqliteRebuildParseFailed:
      "MixLab không đọc được cấu trúc CREATE TABLE của {{table}} đủ rõ để dựng lại. Bảng dùng cú pháp mà MixLab chưa nhận ra.",
    cannotRemoveDirectory: "Không xoá được {{path}}: {{message}}",
    noAppDataDir: "MixLab không có chỗ nào để lưu file riêng của nó: {{message}}",
  },
  followPort: {
    found: "{{count}} kết nối đã lưu vẫn trỏ tới {{service}} ở port {{from}}: {{names}}. Chuyển sang {{to}}?",
    foundOne: "Một kết nối đã lưu vẫn trỏ tới {{service}} ở port {{from}}: {{names}}. Chuyển sang {{to}}?",
    none: "Không có kết nối đã lưu nào trỏ tới {{service}} ở port {{from}}.",
    update: "Cập nhật",
    leave: "Để nguyên",
    ok: "OK",
    updated: "Đã chuyển {{count}} kết nối đã lưu sang {{to}}.",
  },
  dbSync: {
    connections: "Kết nối",
    connectionSecrets: "Mật khẩu kết nối",
    querySnippets: "Đoạn truy vấn",
  },
};

export default dbVi;
