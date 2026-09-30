---
status: implemented
date: 2026-09-05
---

# Hỗ trợ SQL Server (MSSQL)

Ngày: 2026-09-05

## Mục tiêu

Thêm SQL Server làm một `DbKind` đầy đủ trong the standalone client, ngang hàng với MySQL/PostgreSQL/ClickHouse:
kết nối (kể cả qua SSH tunnel có sẵn), duyệt database/schema/table, đọc và sửa dữ liệu qua Data
tab, chạy script tay qua Query tab, sửa cấu trúc qua Structure tab, và dump/restore. Vì đây là một
engine hoàn toàn mới (không phải mở thêm một tính năng trên engine đã có, như các spec ClickHouse
trước), khối lượng việc tương đương với lúc PostgreSQL được thêm vào — spec này vì vậy chia thành
**8 kế hoạch (plan) làm tuần tự**, mỗi plan tự build/test được và đóng góp đúng một lát ngang của
`SqlApi`, giống cách ClickHouse được mở dần qua nhiều spec (`...-ddl-design.md`,
`...-row-writes-design.md`, `...-dump-restore-design.md`, `...-query-dml-design.md`).

Xong toàn bộ 8 plan: mở the standalone client, thêm connection SQL Server (`192.168.50.86:1433`, user `sa`, pass
`admin` — server test hiện có), thấy sidebar liệt kê database/table, mở một bảng thấy dữ liệu phân
trang lọc được, sửa/thêm/xoá dòng, mở Query tab gõ T-SQL nhiều câu (kể cả nhiều batch ngăn bởi
`GO`), mở Structure tab thêm/sửa/xoá cột và index, và Dump/Restore ra file `.sql` chạy lại được.

## Phi mục tiêu (toàn bộ 8 plan)

- **Không Always On / replicas / linked servers / CLR / temporal tables / graph tables.** Đọc và
  ghi nhắm vào bảng thường (`rowstore`, `heap` hoặc có clustered index) trong một database — đúng
  tầm mà MySQL/PostgreSQL/ClickHouse đang phục vụ hôm nay.
- **Không Windows Authentication / Azure AD auth.** Chỉ SQL Server Authentication (user/password) —
  cùng một `ConnectionConfig` shape (`host`/`port`/`username`/`password`/`database`) các engine
  khác đang dùng, không thêm biến thể xác thực mới. [[connectionconfig_shape_decision]] — không phá
  vỡ quyết định giữ chung shape `ConnectionConfig` qua các loại DB.
- **Không hỗ trợ `sequence` (`NEXT VALUE FOR`) làm auto-increment.** Chỉ `IDENTITY(seed, increment)`
  — xem D7. Một cột dùng sequence làm default vẫn đọc/sửa được như cột thường, chỉ không được đánh
  dấu "auto increment" và không có nút reset counter.
- **Không computed column có `PERSISTED`/không PERSISTED khác nhau ở tầng ghi** — cả hai đọc như
  cột generated (giống MySQL's `STORED`/`VIRTUAL` gộp chung), không cho sửa trực tiếp, chỉ drop
  được.
- **Không cố chạy trên Azure SQL Database / Azure SQL Managed Instance trong v1** — nhắm SQL Server
  on-prem/container trước (test server hiện có là SQL Server thường). Azure SQL khác một số DMV và
  hành vi (không có `sys.dm_exec_sessions` đầy đủ quyền, không `KILL` được phiên của người khác trên
  một số gói) — để lại thành việc riêng nếu cần sau.
- **Không hỗ trợ named instance (`host\SQLEXPRESS`) và dynamic port** trong v1. Đây là cấu hình
  rất phổ biến với SQL Server on-prem: instance không nghe cổng 1433 cố định mà đăng ký một cổng
  động, client phải hỏi SQL Browser qua UDP 1434 để biết cổng. `tiberius` có sẵn feature
  `sql-browser-tokio` cho việc này, nhưng `ConnectionConfig` chỉ có `host`/`port` và thêm một field
  `instance` là một thay đổi shape chạm mọi engine ([[connectionconfig_shape_decision]]) — để lại
  thành việc riêng. Người dùng có instance đặt tên vẫn kết nối được nếu bật TCP/IP cổng tĩnh cho
  instance đó, và tài liệu nên nói vậy thay vì im lặng.
- **Không tự bundle driver ODBC hay cài đặt gì lên máy người dùng cho phần đọc/ghi/DDL** (D1) —
  chỉ Plan 7 (dump/restore) có thể cần một tool ngoài, và ngay cả đó cũng ưu tiên tự sinh SQL thay
  vì phụ thuộc tool (D10).
- **Không hỗ trợ toán tử `REGEXP` trong filter bar** — SQL Server không có toán tử regex nào cho
  tới `REGEXP_LIKE` của SQL Server 2025/Azure SQL, nên `regexpFilter: false` giống ClickHouse
  (xem D12).

## Hiện trạng — khuôn mẫu đã có, kế thừa nguyên

| Chỗ | Điều spec dựa vào |
| --- | --- |
| [`src-tauri/src/modules/db/models.rs`](../../apps/desktop/src-tauri/src/modules/db/models.rs) | `DbKind` enum, `ConnectionConfig` (đã đủ field cho MSSQL: host/port/username/password/database/ssh/use_ssl — không cần field mới), `ServerInfo`, `StatementResult`, `SqlProblem` |
| [`src-tauri/src/modules/db/state.rs`](../../apps/desktop/src-tauri/src/modules/db/state.rs) | `DbHandle` enum (thêm biến thể `Mssql`), `ActiveConnection` |
| [`src-tauri/src/modules/db/commands/mod.rs`](../../apps/desktop/src-tauri/src/modules/db/commands/mod.rs) | `connect_db`/`disconnect_db` match theo `DbKind`, `resolve_endpoint` (SSH tunnel — dùng lại nguyên, không đổi), `sql_endpoint` (địa chỉ cho dump/restore), `retry_read!` macro |
| [`src-tauri/src/modules/db/drivers/postgres.rs`](../../apps/desktop/src-tauri/src/modules/db/drivers/postgres.rs) | Mẫu gần MSSQL nhất: `qualify`/`resolve`/`quote_ident` cho tên có schema, `build_where`, `column_value` (decode theo thứ tự thử kiểu), `update_row`/`insert_rows`/`delete_rows` với transaction + pre-check `matched=1` |
| [`src-tauri/src/modules/db/drivers/mysql.rs`](../../apps/desktop/src-tauri/src/modules/db/drivers/mysql.rs) | Mẫu gần MSSQL nhất cho **connection model**: một pool cho cả server (không phải một pool/database như Postgres), `thread_id`/`kill_query` cho cancel |
| [`src-tauri/src/modules/db/drivers/postgres_structure.rs`](../../apps/desktop/src-tauri/src/modules/db/drivers/postgres_structure.rs), `postgres_ddl.rs`, `postgres_script.rs` | Shape `TableStructure`/`StructureColumn`/`TableIndex`/`Collation`/`TableStats` dùng chung cho mọi engine — MSSQL điền vào, không đổi shape |
| [`src-tauri/src/modules/db/drivers/dump.rs`](../../apps/desktop/src-tauri/src/modules/db/drivers/dump.rs), `tools.rs` | Cơ chế tìm/tải tool ngoài (`Tool`/`Suite`) — xem D10 vì sao MSSQL đi khác |
| [`src/modules/db/sql/api.ts`](../../apps/desktop/src/modules/db/sql/api.ts), `sql/dialect.ts`, `types.ts` | `SqlApi`/`SqlDialect`/`SqlEditing` — hợp đồng chung mọi engine SQL phải điền vào, không đổi shape trừ D4 (mở rộng `identifierQuote`) |
| [`src/modules/db/postgres/`](../../apps/desktop/src/modules/db/postgres) (`api.ts`, `dialect.ts`, `columns.ts`, `editing.ts`, `system.ts`) | Bộ file mẫu để copy cấu trúc cho `src/modules/db/mssql/` |
| [`src/modules/db/engines.ts`](../../apps/desktop/src/modules/db/engines.ts), `connectionForm.ts`, `types.ts` | Nơi một `DbKind` mới phải đăng ký: `SQL_ENGINES`, `DEFAULT_PORTS`, `KIND_LABEL`, `hasTls` |
| [`src/modules/db/sql/syntax.ts`](../../apps/desktop/src/modules/db/sql/syntax.ts) | `SqlSyntax` — lexing rules dùng chung cho statement splitter (JS) và bộ tương đương phía Rust; xem D4/D9 vì sao MSSQL cần mở rộng shape này |
| `node_modules/@codemirror/lang-sql` | Đã có sẵn dialect `MSSQL` export riêng, và `SQLConfig.identifierQuotes` đã hỗ trợ `[` cho bracket identifier — không cần tự viết CodeMirror dialect |

## Quyết định kiến trúc chung (áp dụng mọi plan)

**D1 — Driver: [`tiberius`](https://github.com/prisma/tiberius), không phải ODBC.**
`sqlx` (đang dùng cho MySQL/PostgreSQL/SQLite) không có driver MSSQL. Hai lựa chọn thực tế: ODBC
(qua `odbc`/`odbc-api`, cần Driver Manager + Microsoft ODBC Driver cài trên máy người dùng) hoặc
`tiberius` (thuần Rust, tự nói giao thức TDS qua `tokio`, không cần cài gì thêm). Chọn `tiberius` vì
lý do đúng tinh thần app này: mọi driver khác đều không bắt người dùng cài driver hệ thống, và một
ứng dụng Tauri đóng gói sẵn không nên yêu cầu "cài Microsoft ODBC Driver 18 trước" chỉ để mở một kết
nối — đặc biệt trên Linux/macOS nơi driver đó không có sẵn theo mặc định. Cái giá: `tiberius` không
có connection pool tích hợp như `sqlx`, và API của nó là API riêng (không phải `sqlx::Row`) — mỗi
hàm đọc dữ liệu tự viết converter, giống cách `mongo.rs`/`clickhouse.rs` đã tự viết converter cho
driver không phải `sqlx` của chúng.

**D2 — Connection model: giống MySQL (một pool cho cả server), không giống PostgreSQL.**
Một kết nối SQL Server *có thể* đổi database đang dùng bằng `USE [db]` hoặc tham chiếu ba phần
`[db].[schema].[table]`, giống hệt MySQL và khác PostgreSQL (nơi một kết nối bị khoá cứng vào một
database). Vì vậy `DbHandle::Mssql` chỉ cần bọc một pool duy nhất — không cần `Pools`
(HashMap theo tên database) như `postgres::Pools`. `database` trong mọi lệnh MSSQL đóng vai trò y
hệt MySQL's `database`: tên để `USE` hoặc để ghép vào câu SQL, không phải để chọn pool.

Vì `tiberius` không có pool sẵn, `Pool` ở đây là một pool tự dựng. **Chốt lúc code Plan 1:
`deadpool` 0.12.3 trần**, không phải `deadpool-tiberius` — `Manager` của deadpool 0.12 chỉ đòi
`create` và `recycle`, nên bọc `tiberius::Client` là hai hàm chứ không đáng thêm một crate cầu nối
nữa vào cây phụ thuộc. `recycle` chạy `SELECT 1`: một connection chết được bỏ đi và quay số lại,
đó là thứ làm một tunnel rớt rồi lên lại dùng được tiếp mà không phải connect tay. `max_size = 8`.

**D3 — Schema mặc định là `dbo`, tái dùng nguyên khuôn `qualify`/`resolve` của PostgreSQL.**
SQL Server có `database > schema > table`, giống PostgreSQL và khác MySQL (chỉ có
`database > table`). `dbo` đóng đúng vai trò `public` bên Postgres: là schema mặc định của mọi user
mới, nên một bảng thuộc `dbo` hiển thị không tiền tố, bảng thuộc schema khác hiển thị
`schema.table`. Copy nguyên `postgres::qualify`/`resolve`/`split_qualified`/`needs_quoting` sang
`mssql.rs`, đổi hằng `DEFAULT_SCHEMA = "dbo"` và đổi `quote_ident` theo D4.

**D4 — Định danh dùng ngoặc vuông `[ ]`, không phải backtick hay `"`. Cần mở rộng `SqlSyntax`.**
`quote_ident` của MSSQL là `[name]` (đóng ngoặc `]` bên trong nhân đôi thành `]]`), không phải một
ký tự đối xứng như ba engine kia. `SqlSyntax.identifierQuote: string | null` hiện giả định ký tự mở
= ký tự đóng — đúng cho backtick và `"` nhưng sai cho `[`/`]`.

Chọn: đổi `identifierQuote` thành một cặp `{ open: string; close: string } | null` (khi
`open === close`, hành vi y hệt hôm nay). Ba hằng số hiện có phải sửa —
`MYSQL_SYNTAX`/`SQLITE_SYNTAX`/`CLICKHOUSE_SYNTAX` đều đang là `` "`" `` và thành
`` { open: "`", close: "`" } ``; chỉ `POSTGRES_SYNTAX` là `null` (nó đọc `"` qua nhánh
`doubleQuoteIsIdentifier` chứ không qua field này).

**Bốn chỗ đọc field này, không phải một** — cả bốn đều ở frontend, `identifierQuote` không tồn tại
phía Rust:

- `src/modules/db/sql/statements.ts:126,131` — bộ tách câu.
- `src/modules/db/sql/lint.ts:154,160` — tokenizer của bộ kiểm tra.
- `src/modules/db/sql/lint.ts:344` (`asWritten`) — bọc một tên gợi ý bằng *một* ký tự và nhân đôi
  chính nó để escape. Với `[`/`]` nó sẽ sinh ra `[name[`; hàm này phải viết lại theo cặp
  open/close, escape bằng cách nhân đôi ký tự **đóng**.

Phía Rust không có `SqlSyntax`: `mysql_script.rs`/`postgres_script.rs` hard-code luật lexing của
riêng chúng. `mssql_script.rs` vì vậy viết splitter riêng (Plan 5), và giữ đồng bộ với bản JS bằng
test song song chứ không bằng một shape dùng chung.

Đây là thay đổi *shape* duy nhất chạm vào các engine cũ. **Nó phải đứng ở Plan 4, trước Query tab
(Plan 5), không phải ở cuối** — xem hộp bên dưới.

> **Vì sao không hoãn tới plan cuối được.** Bộ tách câu chạy ở **frontend**, không phải backend:
> `QueryEditor.tsx:430` và `:735` gọi `splitStatements(text, dialect.syntax)` để quyết định gửi gì
> lên server và để `guard.ts` xét statement nào là write. Nếu Query tab mở ra khi `MSSQL_SYNTAX`
> chưa hiểu `[ ]` và `GO`: `SELECT * FROM [Order;Details]` bị cắt làm hai câu và gửi SQL rác lên
> server; `GO` được gửi nguyên như một statement và luôn lỗi cú pháp — dù `mssql_script.rs` phía
> Rust có tách batch đúng thì cũng vô nghĩa, vì frontend đã cắt sai trước đó. Plan 1-3 (chỉ Data
> tab, không có Query tab) thì hoàn toàn không cần nó, nên đây là chỗ đúng để cắt.

CodeMirror không cần việc này: `@codemirror/lang-sql` đã có `MSSQL` dialect dựng sẵn và
`SQLConfig.identifierQuotes` nhận `"\"["` để hiểu cả `"` lẫn `[` — chỉ cần truyền đúng string đó khi
gọi `sql({ dialect: MSSQL, ... })`, không cần viết dialect mới.

**D5 — Việc đăng ký `DbKind::Mssql` mới, checklist dùng chung cho mọi plan bên dưới (mỗi plan chỉ
động tới phần của mình):**

Backend:
- `models.rs`: thêm `Mssql` vào `enum DbKind` (`#[serde(rename_all = "lowercase")]` → serialize
  thành `"mssql"`).
- `state.rs`: thêm biến thể `DbHandle::Mssql(Pool)` (kiểu `Pool` xem D2).
- `commands/mod.rs`: thêm nhánh `DbKind::Mssql` trong `connect_db` (Plan 1), `disconnect_db`
  (Plan 1), một hàm `mssql_pool(state, id)` cạnh `mysql_pool`/`postgres_pool` (Plan 1), nhánh trong
  `sql_endpoint` cho dump/restore (Plan 7).
- `drivers/mod.rs`: `pub mod mssql; pub mod mssql_ddl; pub mod mssql_dump; pub mod mssql_script;
  pub mod mssql_structure;` (chia file y hệt cách postgres chia bốn file, cộng một `_dump.rs` riêng
  giống `clickhouse_dump.rs`/`sqlite_dump.rs` vì dump ở đây tự sinh SQL — xem D10 — không dồn hết
  vào một file nghìn dòng).
- `commands/mod.rs` (khai module) + `modules/mod.rs`: `pub mod mssql;` và các dòng
  `generate_handler!` cho từng lệnh mới.

Frontend:
- `types.ts`: `DbKind` union thêm `"mssql"`, `DEFAULT_PORTS.mssql = 1433`.
- `sql/dialect.ts`: `SqlDialect.kind` là một union literal **riêng**
  (`"mysql" | "postgres" | "sqlite" | "clickhouse"`, `dialect.ts:73`) — phải nới thêm `"mssql"`
  cùng lúc với `types.ts`, nếu không `mssqlDialect` không type-check.
- `engines.ts`: `SQL_ENGINES.mssql = { api: mssqlApi, dialect: mssqlDialect }`.
- `connectionForm.ts`: `KIND_LABEL.mssql`, `hasTls` thêm `"mssql"` (D6 — MSSQL có encryption
  option, box TLS có ý nghĩa).
- `i18n/en.ts`, `i18n/vi.ts`: `connection.kindMssql`, `error.mssql`.
- `src/modules/db/mssql/`: `api.ts`, `dialect.ts`, `columns.ts`, `editing.ts`, `system.ts` — copy
  cấu trúc thư mục `postgres/`.

**Ba chỗ nữa mà bản nháp đầu của checklist này bỏ sót** — `tsc` tìm ra lúc code Plan 1, nên chúng
được ghi lại ở đây thay vì để plan sau lại vấp:

- `src/modules/db/icons.tsx`: `BRAND_MARKS` là một `Record<DbKind, …>`, nên mỗi kind mới phải có
  một mark. Có sẵn fallback `DatabaseGenericIcon` nhưng chỉ cho kind *lạ* đọc từ
  `connections.json`, không phải cho một kind build này biết. Lưu ý tiền lệ trong chính file đó:
  `sqlite` và `clickhouse` **không** phải brand mark mà là hình tự vẽ, có comment nói rõ — nên vẽ
  một hình cho MSSQL là đúng quy ước, miễn là không giả làm logo Microsoft và nói thẳng điều đó.
- `DatabaseActions.tsx` và `DatabaseStats.tsx` mỗi cái có một union `kind` **riêng**
  (`"mysql" | "postgres" | "mongo" | "sqlite" | "clickhouse"`), không đọc `DbKind` — cả hai phải
  thêm `"mssql"`.
- Trong `DatabaseActions.tsx`, `const suite: ToolSuite | null = …` phải cho MSSQL về `null` cùng
  nhóm với SQLite/ClickHouse (D10: dump tự thân, không có suite tool ngoài để đặt tên). Không làm
  thì `ToolSuite` không nhận `"mssql"` và `tsc` chặn.

Mỗi plan bên dưới nói rõ nó cần *bao nhiêu* trong checklist này để tự chạy được (Plan 1 cần gần hết
để app build và connect được; các plan sau chỉ thêm method/lệnh mới, không đụng lại phần khung).

**Feature của `tiberius` trong `Cargo.toml`** — không chỉ `tokio` + TLS. D11 hứa đọc được
`DECIMAL`/`DATE`/`TIME`/`DATETIME2`/`DATETIMEOFFSET`, và mỗi thứ đó là một feature gate: `tds73`
(giao thức TDS 7.3 trở lên — không có nó thì server hạ cấp và các kiểu date/time mới **không có
biến thể `ColumnData` tương ứng**), `rust_decimal` (hoặc `bigdecimal`) cho `DECIMAL`/`NUMERIC`,
`chrono` (hoặc `time`) cho nhóm ngày giờ. Bật thiếu thì D11 không thực hiện được chứ không phải
hiển thị xấu. TLS thì `tiberius` có sẵn `native-tls`, nên không cần ngoại lệ so với stack hiện có.

**D6 — TLS/encryption: `use_ssl` giữ nguyên nghĩa, map sang `EncryptionLevel` của `tiberius`.**
`tiberius::Config::encryption(EncryptionLevel)` nhận bốn giá trị, và **tên của chúng đánh lừa** —
đọc trong `tiberius-0.12.3/src/tds.rs` lúc code Plan 1, không phải suy từ tên:

| Giá trị | Nghĩa thật |
| --- | --- |
| `Off` | *Chỉ* mã hoá thủ tục đăng nhập |
| `On` | Mã hoá tất cả nếu có thể |
| `NotSupported` | **Không** mã hoá gì cả |
| `Required` | Mã hoá tất cả, hỏng thì báo lỗi |

Nghĩa là "tắt TLS" là `NotSupported` chứ không phải `Off`. Bản nháp đầu của spec này viết `Off` =
bỏ TLS, và điều đó sai — nhưng `Off` **vẫn là lựa chọn đúng** cho `use_ssl == Some(false)`: ý của
ô đó là "đã tunnel qua SSH rồi thì khỏi TLS lần hai", và gói đăng nhập vẫn được bảo vệ là điều tốt
hơn chứ không phải điều phải bỏ. `None`/`Some(true)` → `Required` — cứng hơn `Prefer` của Postgres,
nhưng đúng thực tế: SQL Server *luôn* mã hoá gói đăng nhập dù server không bật TLS đầy đủ, nên "thử
TLS rồi rơi về plaintext" không phải lựa chọn nhị phân sạch như hai engine kia.

**Chốt lúc code Plan 1: `trust_cert()` luôn bật, không thêm ô riêng.** Certificate tự ký là thứ
một instance tự cài luôn có, và từ chối nó thì app không với tới được đúng loại server nó hay được
chĩa vào nhất. Nghĩa là hộp TLS của MSSQL hiểu ngầm "mã hoá, **không** xác minh chuỗi CA" — khác
với nghĩa của cùng cái hộp đó trên MySQL/PostgreSQL, và đó là lý do việc này được ghi vào doc
comment của `mssql::connect` chứ không chỉ ở đây.

**D7 — "Tự tăng" (auto-increment) = cột `IDENTITY(seed, increment)`.**
Đọc từ `sys.identity_columns` (has columns `seed_value`, `increment_value`, `last_value`) join
`sys.columns`. Ánh xạ vào field chung: `SqlColumnMeta.extra`/`SqlStructureColumn.autoIncrement` đọc
y hệt MySQL's `auto_increment` — cột có trong `sys.identity_columns` → `autoIncrement = true`.
Reset counter sau khi xoá hết dữ liệu (`resetAutoIncrement`) dùng `DBCC CHECKIDENT` — tương đương
`ALTER TABLE ... AUTO_INCREMENT = 1` của MySQL, nhưng với hai khác biệt phải xử lý, không gọi vô
điều kiện như MySQL:

- **Tên bảng là một chuỗi có schema, không phải định danh.** `DBCC CHECKIDENT ('dbo.mytable',
  RESEED, 0)` — bỏ schema thì lệnh chỉ trúng khi bảng thuộc schema mặc định của user, và cú pháp
  `[ ]` của D4 không dùng ở đây (đây là tham số chuỗi, escape bằng nhân đôi `'`).
- **Bảng không có cột IDENTITY thì lệnh báo lỗi**, trong khi `AUTO_INCREMENT = 1` của MySQL vô
  hại. Phải kiểm `sys.identity_columns` trước và bỏ qua lặng lẽ nếu không có — `deleteRows(all =
  true, resetAutoIncrement = true)` được gọi trên bảng bất kỳ.

Một cột `rowversion`/`timestamp` cũng do server tự gán và **không insert/update được**, dù nó không
phải IDENTITY. Nó phải rơi vào `isServerAssigned` (xem `postgres/columns.ts:22` làm mẫu: hàm đó là
`isAutoIncrement || isGenerated`, MSSQL cần thêm vế thứ ba theo `dataType`), nếu không mọi INSERT
từ grid trên bảng có cột như vậy sẽ lỗi.

**D8 — Cancel một script đang chạy: cần xác minh API `tiberius` lúc code Plan 5, không chốt cứng ở
đây.** MySQL huỷ bằng `KILL QUERY <thread_id>` (dừng câu lệnh, giữ session), PostgreSQL bằng
`pg_cancel_backend(pid)` (tương tự). SQL Server có `KILL <session_id>` qua T-SQL, nhưng đó là lệnh
**đóng luôn cả session** chứ không có "KILL QUERY" tách riêng dừng-statement-giữ-session — nếu đúng
vậy thì cancel trên MSSQL sẽ nặng tay hơn ba engine kia (mất luôn transaction/temp table của session
đang chạy, không chỉ câu lệnh). Đường đúng hơn cho "dừng một câu lệnh, giữ session" là gói TDS
`Attention` mà driver client (SSMS, hay chính `tiberius`) có thể gửi trên cùng kết nối đang chạy —
nếu `tiberius` expose được API này (cần đọc doc/source của crate lúc code, không giả định ở đây),
đó là lựa chọn ưu tiên; nếu không, rơi về `KILL <session_id>` qua một connection phụ (giống mẫu
MySQL/Postgres), chấp nhận cái giá "cancel = mất session" và ghi rõ trong doc comment của
`dialect.cancellable`.

**Cái bẫy của nhánh `KILL`: nó là chuyện quyền, không chỉ chuyện nặng tay.** `KILL` đòi
`ALTER ANY CONNECTION` (hoặc `sysadmin`/`processadmin`) — khác hẳn `pg_cancel_backend` của
PostgreSQL, vốn luôn cho phép huỷ backend của chính mình. Nghĩa là một login thường **không huỷ
được ngay cả session của chính nó**. Server test dùng `sa` nên sẽ chạy được và che mất vấn đề này;
đừng lấy đó làm bằng chứng là xong. Nếu phải rơi về `KILL`, chọn một trong hai và ghi vào spec lúc
code Plan 5: hoặc thử quyền một lần lúc connect và đặt `cancellable` theo kết quả, hoặc để nút
Cancel luôn bật và trả nguyên lỗi permission của server cho người dùng đọc.

**D9 — `GO` là dấu ngăn batch phía client, không phải cú pháp SQL thật — ảnh hưởng bộ tách câu.**
T-SQL script thường ngăn cách các "batch" bằng dòng chỉ chứa `GO` (không phải `;` — `;` vẫn ngăn
statement như bình thường *trong* một batch). `GO` không phải từ khoá SQL: gửi nguyên `GO` cho
server sẽ lỗi cú pháp. Bộ tách câu hiện tại (`mysql_script.rs`/`postgres_script.rs` phía Rust,
`sql/statements.ts` phía JS) tách theo `;` và không biết khái niệm batch. `mssql_script::run` cần
một bước tách batch-theo-`GO` **trước**, rồi trong mỗi batch mới tách câu theo `;` như các engine
khác — hai tầng tách riêng, không nhét `GO` vào cùng bộ tách `;` hiện có (rủi ro: một chuỗi hoặc
comment chứa chữ `go` không được nhầm là dấu ngăn — chỉ một dòng *chỉ có* `GO`, có thể kèm số lặp
`GO 3`, mới được coi là batch separator — đúng luật SQL Server thật). Mỗi batch chạy như một round
trip riêng tới server (giống thật: `GO` vốn dĩ là chỗ SSMS/`sqlcmd` cắt và gửi từng phần).

**D10 — Dump/restore: không có tool tương đương `pg_dump`/`mysqldump` miễn phí, dễ tải, dễ pin
version từ Microsoft.** `sqlcmd` (nay là bản Go, mã nguồn mở, tải qua GitHub release — có sẵn
build cho cả ba platform) chạy được script `.sql` — dùng được cho **restore**, đóng đúng vai trò
`psql`/`mysql` client hiện tại (xem `tools.rs::Tool::PsqlClient`/`MysqlClient`). Nhưng không có
tương đương `pg_dump`/`mysqldump` cho chiều **dump** (`bcp` chỉ xuất dữ liệu thô một bảng ở định
dạng riêng, không sinh DDL + INSERT thành file `.sql` đọc được). Đề xuất: viết **dump tự thân bằng
`tiberius`** — không tool ngoài — sinh DDL (`CREATE TABLE`, `CREATE INDEX`...) bằng cách đọc lại
catalog views (Plan 2 đã có sẵn code đọc structure để tái dùng) và sinh `INSERT` bằng cách đọc dữ
liệu theo trang (giống cách Plan 2 đã đọc).

Đây **không** phải chuyện chưa có tiền lệ trong repo này:
[`clickhouse_dump.rs`](../../apps/desktop/src-tauri/src/modules/db/drivers/clickhouse_dump.rs) và
[`sqlite_dump.rs`](../../apps/desktop/src-tauri/src/modules/db/drivers/sqlite_dump.rs) đã tự sinh dump không
tool ngoài. `mssql_dump.rs` lấy `clickhouse_dump.rs` làm khuôn — cùng `dump::Tracker`, cùng
`TRANSFER_PROGRESS_EVENT` — chứ không phát minh lại cách báo tiến độ.

Restore vẫn có thể dùng `sqlcmd` nếu tìm/tải được, hoặc cũng tự thân nếu `sqlcmd` không tải được
trên một platform nào đó. **Quyết định cuối (dùng `sqlcmd` cho restore hay tự thân luôn cả hai
chiều) để ngỏ tới Plan 7** — cần hỏi lại sau khi Plan 1-6 xong, xem giá tự viết dump/restore
round-trip đáng tin tới đâu so với công sức thêm một `Suite::Mssql` vào `tools.rs`.

**D11 — Kiểu dữ liệu: thứ tự thử decode trong `column_value`, giống `postgres::column_value`.**
`tiberius` trả `ColumnData<'_>` là một enum Rust đã gõ kiểu theo cột (giống Postgres, khác cách
MySQL suy kiểu từ byte) — nghĩa là hàm chuyển sang `serde_json::Value` là một `match` trên
`ColumnData` chứ không phải chuỗi `try_get::<T>()` như Postgres. Ánh xạ dự kiến (chốt chi tiết lúc
code Plan 1, sau khi xác nhận đúng tên biến thể của `tiberius::ColumnData` tại version dùng):
`BIT→bool`, `TINYINT/SMALLINT/INT/BIGINT→number`, `REAL/FLOAT→number` (NaN/Infinity → null, giống
lý do Postgres làm vậy — xem `is_decodable`), `DECIMAL/NUMERIC/MONEY/SMALLMONEY→string` (giữ
precision, giống Postgres's `rust_decimal`), `DATE/TIME/DATETIME/DATETIME2/SMALLDATETIME/
DATETIMEOFFSET→string` (ISO-ish, giống format Postgres đang trả), `UNIQUEIDENTIFIER→string`,
`VARCHAR/NVARCHAR/CHAR/NCHAR/TEXT/NTEXT→string`, `VARBINARY/BINARY/IMAGE→base64 string` (giống
`Vec<u8>` case của Postgres). `sql_variant`, `xml`, `geography`/`geometry`, `hierarchyid` đọc như
text nếu server cho phép ép `CAST(col AS nvarchar(max))`, giống cách Postgres text-hoá kiểu không có
decoder riêng.

**D12 — Filter bar: `regexpFilter: false`, và `escape_like` phải có bản riêng cho MSSQL.**
`build_where` "copy khuôn Postgres" đúng ở phần khung nhưng sai ở ba toán tử, cả ba đều là khác
biệt thật của T-SQL chứ không phải khác cách viết:

- **Không có regex.** SQL Server không có toán tử nào tương đương `~` của PostgreSQL hay `REGEXP`
  của MySQL cho tới `REGEXP_LIKE` của SQL Server 2025. `mssqlDialect.regexpFilter = false`, giống
  `clickhouse/dialect.ts:57` — một toán tử không bao giờ chạy được thì không nên có trong dropdown.
  `build_where` cũng không cần nhánh cho nó, y hệt `sqlite.rs`.
- **`LIKE` của SQL Server không có escape character mặc định.** `filters.rs::escape_like` dùng `\`
  và doc comment của chính nó nói rõ vì sao nó là *một* hàm cho hai engine: "MySQL và PostgreSQL
  đều lấy `\` làm escape mặc định". SQL Server thì không — `\` chỉ là một ký tự thường, nên phải
  viết `ESCAPE '\'` tường minh vào **mọi** câu `LIKE` sinh ra.
- **`[` là ký tự đại diện trong `LIKE` của T-SQL** (`[a-c]` là một tập ký tự), thứ không engine nào
  khác có. `escape_like` hiện escape `\ % _` và để lọt `[`, nên filter "contains" cho chuỗi `a[0]`
  sẽ trả sai kết quả một cách im lặng.

Chọn: một hàm `escape_like_mssql` riêng trong `mssql.rs` (không sửa `filters.rs` dùng chung — hàm
đó đang đúng cho hai engine nó phục vụ), escape `\ % _ [`, và mọi `LIKE`/`NOT LIKE` sinh ra đều
kèm `ESCAPE '\'`. Thêm test cho `a[0]` và `50%` giống các test đã có ở cuối `filters.rs`.

Cuối cùng, **không có `ILIKE`**: so sánh phân biệt hoa thường hay không là do collation của cột
quyết định (`*_CI_*` là mặc định của đa số cài đặt, nên trên thực tế filter sẽ *không* phân biệt
hoa thường). Không ép bằng `LOWER()` — nó phá index. Ghi hành vi này vào doc comment của
`build_where` vì nó khác cả ba engine kia.

**D13 — Isolation level: Data tab đọc phải có `LOCK_TIMEOUT`, nếu không nó sẽ treo im lặng.**
SQL Server mặc định chạy `READ COMMITTED` **có khoá**, không phải MVCC như PostgreSQL và InnoDB.
Hệ quả cụ thể: mở một bảng đang bị transaction khác giữ khoá ghi thì `SELECT` **chờ vô hạn** —
không lỗi, không timeout, spinner quay mãi. Ba engine hiện có không bao giờ hành xử như vậy, nên
đây không phải "SQL Server chậm" mà là một khác biệt phải xử lý ở tầng driver.

Chọn: mọi connection đi ra từ pool đặt `SET LOCK_TIMEOUT 5000` ngay khi mở (5 giây — đủ để một
transaction ngắn đi qua, đủ ngắn để người dùng không tưởng app treo), và lỗi 1222 "Lock request
time out" được trả nguyên văn lên UI. Đường **chỉ đọc** (`table_data`, `table_structure`,
`table_stats`, `schema_outline`, `list_tables`) thêm `SET TRANSACTION ISOLATION LEVEL READ
UNCOMMITTED` — duyệt dữ liệu để xem không đáng để chặn người khác ghi, và đây đúng là cái mọi công
cụ cùng loại làm. Đường **ghi** (`update_row`/`insert_rows`/`delete_rows`, DDL, script tay) giữ
nguyên `READ COMMITTED` mặc định: một dòng đọc bẩn rồi ghi đè là chuyện khác hẳn với một dòng đọc
bẩn rồi hiển thị. Ghi rõ sự chia đôi này trong doc comment của `mssql::connect`.

**D14 — `objectCollation`: SQL Server có collation cấp database nhưng không có cấp bảng, mà cờ
này gate cả hai.** `SqlEditing.objectCollation` (`sql/dialect.ts:46`) là "một database **hoặc một
bảng** mang collation của riêng nó", và nó được đọc ở đúng hai chỗ:
`DatabaseDialog.tsx:38` và `TableDialog.tsx:84`. Lưu ý collation **cấp cột** luôn được chào và
không đi qua cờ này (`ColumnDialog` đọc `collations` trực tiếp) — nên PostgreSQL, nơi chỉ cột mới
có collation, để `objectCollation: false` (`postgres/editing.ts:74`).

SQL Server nằm giữa: `CREATE DATABASE [x] COLLATE Vietnamese_CI_AS` là hợp lệ và là một thuộc tính
quan trọng của database, còn `CREATE TABLE` thì **không có mệnh đề `COLLATE` cấp bảng** nào cả.
Đặt `true` sẽ chào một ô collation trong TableDialog mà `create_table` không có chỗ để dùng; đặt
`false` thì mất luôn ô collation lúc tạo database, dù `SqlApi.createDatabase(id, name, collation)`
(`sql/api.ts:137`) đã sẵn tham số cho nó.

Chọn: **tách cờ thành `databaseCollation` và `tableCollation`** trong `SqlEditing`, MySQL đặt cả
hai `true`, PostgreSQL/ClickHouse cả hai `false` (hành vi không đổi), MSSQL `databaseCollation:
true` / `tableCollation: false`. Đây là thay đổi shape thứ hai chạm các engine cũ, nhưng nhỏ hơn
D4 nhiều (một field tách đôi, hai call site) và nó thuộc Plan 6 (DDL) — đúng plan cần nó. Kèm
theo: `create_database` phải thật sự sinh `COLLATE` khi tham số khác `null`, việc mà bản nháp
trước của spec này bỏ quên.

**D15 — `ALTER COLUMN` của SQL Server không phải một câu lệnh, mà là một chuỗi lệnh.**
Đây là chỗ MSSQL lệch MySQL/PostgreSQL nhiều nhất trong toàn bộ spec này. `ALTER TABLE ... ALTER
COLUMN` **bị server từ chối** khi cột đang:

- có default constraint gắn vào — và tên constraint thường do server tự sinh (`DF__t__col__1A2B3C4D`),
  nên phải tra `sys.default_constraints` mới biết mà `DROP CONSTRAINT`;
- nằm trong một index, PRIMARY KEY hay UNIQUE constraint — phải drop index, alter, tạo lại;
- có check constraint, hoặc bị một FOREIGN KEY tham chiếu tới.

Thêm hai cái bẫy nữa: `ALTER COLUMN` **thay thế toàn bộ định nghĩa cột**, nên quên viết lại
`NOT NULL` là cột lặng lẽ thành nullable và quên `COLLATE` là mất collation; và **không có cách nào
bật/tắt `IDENTITY` bằng `ALTER COLUMN`** (đổi được chỉ bằng cách tạo bảng mới rồi copy).

Chọn: `mssql_ddl::modify_column` là một **chuỗi lệnh chạy trong một transaction**, không phải một
câu — đọc constraint/index hiện có của cột từ catalog → drop những cái chặn → `sp_rename` nếu tên
đổi → `ALTER COLUMN` với định nghĩa **đầy đủ** dựng lại từ `SqlColumnSpec` (kiểu, nullable,
collation — mọi thứ, kể cả phần người dùng không sửa) → dựng lại default constraint và index đã
drop. `drop_column` cũng phải drop default constraint trước. Phi mục tiêu của D15: **không** đổi
được IDENTITY on/off qua Structure tab — nút đó phải bị khoá với thông báo rõ, chứ không phải để
người dùng bấm rồi nhận lỗi server.

## Kế hoạch triển khai — 8 plan

Mỗi plan là một PR/commit-set độc lập, build xanh và test qua được sau khi làm xong, không để dở
dang giữa plan. Thứ tự là bắt buộc: plan sau dựa vào file/hàm plan trước tạo ra.

### Plan 1 — Khung kết nối: connect/disconnect, server info, list databases/tables

**Phạm vi:** `DbKind::Mssql` tồn tại và connect được thật (kể cả qua SSH tunnel), sidebar liệt kê
được database và table, header hiện version server — **chưa có Data tab** (bảng mở ra rỗng hoặc
báo "not implemented" tạm), chưa Query/Structure tab.

**Backend:**
- `Cargo.toml`: thêm `tiberius` (+ pool crate theo D2) với đủ feature theo D5 — `tokio`,
  `native-tls`, `tds73`, `rust_decimal`, `chrono`. Ba cái sau không phải tuỳ chọn: thiếu chúng thì
  D11 không có biến thể `ColumnData` để match.
- `drivers/mssql.rs`:
  - `pub async fn connect(host, port, username, password, database, use_ssl) -> Result<Pool,
    AppError>` (D1/D2/D6), đặt `SET LOCK_TIMEOUT` cho mỗi connection mới theo D13.
  - `pub async fn server_info(pool) -> Result<ServerInfo, AppError>` — **đọc
    `SERVERPROPERTY('ProductVersion')`, `SERVERPROPERTY('Edition')` và
    `SERVERPROPERTY('ProductLevel')`, không cắt chuỗi `@@VERSION`.** `@@VERSION` được **localize**
    theo ngôn ngữ cài đặt của server, nên parse nó ra rác trên bất kỳ server không phải tiếng Anh
    nào; `SERVERPROPERTY` trả giá trị máy đọc được và ổn định. `@@VERSION` chỉ dùng để lấy phần tên
    OS ở cuối chuỗi, và hỏng ở đó thì `ServerInfo.os` để rỗng chứ không làm hỏng cả lệnh.
  - `pub async fn list_databases(pool) -> Result<Vec<String>, AppError>`: `SELECT name FROM
    sys.databases WHERE database_id > 4 AND state = 0 AND HAS_DBACCESS(name) = 1 ORDER BY name` —
    `database_id > 4` loại 4 database hệ thống `master/tempdb/model/msdb`, `state = 0` loại
    database offline/đang restore, và **`HAS_DBACCESS(name) = 1` loại database mà login hiện tại
    không mở được**. Vế thứ ba là vế dễ quên nhất vì server test dùng `sa` (thấy mọi thứ): thiếu
    nó, một login thường thấy database trong sidebar rồi nhận lỗi "server principal is not able to
    access the database" khi bấm vào.
  - `pub async fn list_tables(pool, database) -> Result<Vec<String>, AppError>`: **liệt kê cả bảng
    lẫn view**, vì `postgres::list_tables` cũng vậy (`relkind IN ('r','p','v','m','f')`, và doc
    comment của nó nói rõ "Every table **and view**") — dùng `sys.objects WHERE type IN ('U','V')`
    join `sys.schemas`, không dùng `sys.tables` (chỉ có bảng, mất hết view). `ORDER BY (s.name <>
    'dbo'), s.name, o.name`. Không cần lọc schema hệ thống như `system_schema_filter` bên Postgres.
  - `qualify`/`resolve`/`quote_ident` (D3/D4, copy từ `postgres.rs`, đổi `DEFAULT_SCHEMA = "dbo"`
    và quote thành `[ident]`/`]]`).
- `state.rs`: `DbHandle::Mssql(Pool)`.
- `commands/mssql.rs` (mới): `mssql_list_databases`, `mssql_server_info`, `mssql_list_tables` —
  ba lệnh, chữ ký y hệt `postgres_list_databases`/`postgres_server_info`/`postgres_list_tables`.
- `commands/mod.rs`: nhánh `DbKind::Mssql` trong `connect_db` (dùng `resolve_endpoint` có sẵn —
  không đổi gì ở tầng SSH tunnel) và `disconnect_db` (đóng pool), hàm `mssql_pool(state, id)`.
- `modules/mod.rs`: khai `mod mssql;` (commands), đăng ký 3 lệnh vào `generate_handler!`.

**Frontend:**
- `types.ts`: `DbKind` thêm `"mssql"`, `DEFAULT_PORTS.mssql = 1433`.
- `src/modules/db/mssql/api.ts` (mới): chỉ điền `listDatabases`/`listTables`/`serverInfo`, mọi
  method khác tạm `notSupported()` (giống cách `postgresApi` đóng 5 method ClickHouse-only lúc
  đầu) — **không** thêm vào `SQL_ENGINES` (`engines.ts`) ở plan này, vì `isSqlKind` gate cả Data
  tab lẫn sidebar bảng — chờ tới khi Plan 2 xong table data mới bật, tránh mở workspace ra một bảng
  không đọc được gì.
- Sidebar và connection form: `KIND_LABEL.mssql`, `hasTls` thêm `"mssql"`, form thêm "SQL Server"
  vào danh sách chọn kind.

**Tiêu chí xong:** thêm connection SQL Server test (`192.168.50.86:1433`, `sa`/`admin`) từ UI,
bấm Connect thành công, sidebar hiện đúng danh sách database khác `master/tempdb/model/msdb`, chọn
một database hiện đúng danh sách table.

### Plan 2 — Đọc bảng: table data, structure, schema outline, collations

**Phạm vi:** Data tab và Structure tab **đọc** được (chưa ghi). `SQL_ENGINES.mssql` được thêm vào
`engines.ts` ở plan này — từ đây một bảng MSSQL mở ra trong workspace thật.

**Backend (`drivers/mssql.rs` + `drivers/mssql_structure.rs` mới):**
- `table_columns`/`ColumnMeta` (giống Postgres's shape): đọc từ `sys.columns` join
  `sys.types`/`INFORMATION_SCHEMA.COLUMNS` (cần cả hai — `sys.columns` có `is_identity`/
  `is_computed`/`is_nullable` dạng bit chuẩn, `INFORMATION_SCHEMA.COLUMNS` có
  `DATA_TYPE`/`CHARACTER_MAXIMUM_LENGTH`/`NUMERIC_PRECISION` dễ ghép thành chuỗi kiểu hiển thị kiểu
  `varchar(255)`/`decimal(10,2)` giống MySQL/Postgres đang hiển thị). Default value đọc qua
  `sys.default_constraints` (`definition` column, dạng `((0))`/`(getdate())` — bọc thêm ngoặc so
  với Postgres, cần bóc ngoặc ngoài khi hiển thị, giữ để hiển thị y hệt SSMS khi ghi lại DDL).

  **Hai quy ước của catalog SQL Server làm chuỗi kiểu hiển thị sai nếu đọc thẳng:**
  - `sys.columns.max_length` đo bằng **byte**, không phải ký tự. Với `nchar`/`nvarchar` (2
    byte/ký tự) thì `nvarchar(255)` đọc ra `510` — phải chia đôi cho nhóm Unicode trước khi ghép
    chuỗi. `CHARACTER_MAXIMUM_LENGTH` của `INFORMATION_SCHEMA` thì đã là ký tự, nên chọn một nguồn
    và dùng nhất quán chứ đừng trộn.
  - Giá trị `-1` (ở cả `max_length` lẫn `CHARACTER_MAXIMUM_LENGTH`) nghĩa là **`MAX`**, không phải
    độ dài âm — phải sinh `varchar(max)`/`nvarchar(max)`/`varbinary(max)`, chứ không phải
    `varchar(-1)`.
- `rowversion`/`timestamp` phải được đánh dấu server-assigned (D7) để INSERT không bao giờ nêu tên
  chúng.
- Foreign key: `sys.foreign_key_columns` join `sys.foreign_keys`, `sys.tables`, `sys.columns` —
  cùng shape `ForeignKey { table, column }` của Postgres.
- `primary_key`: `sys.indexes WHERE is_primary_key = 1` join `sys.index_columns`.
- `build_where`: copy khuôn Postgres cho phần khung, nhưng theo **D12** cho ba toán tử lệch —
  không có nhánh `regexp`/`notRegexp` (dialect đã đóng chúng), `escape_like_mssql` escape thêm `[`,
  và mọi `LIKE` sinh ra kèm `ESCAPE '\'`. Test kèm theo: filter "contains" cho `a[0]` và cho `50%`
  phải trả đúng dòng chứa đúng chuỗi đó.
- `table_data`: COUNT trước rồi SELECT phân trang như Postgres, nhưng phân trang dùng
  `OFFSET ... ROWS FETCH NEXT ... ROWS ONLY` (cú pháp SQL Server 2012+, không phải `LIMIT`/`OFFSET`)
  — và cú pháp này **bắt buộc có `ORDER BY`**, khác Postgres/MySQL cho phép `LIMIT` trần.

  Khi người dùng không chọn cột sort, dùng **`ORDER BY (SELECT NULL)`**, không phải khoá chính và
  càng không phải "cột đầu tiên". `(SELECT NULL)` thoả cú pháp mà không ép server sort gì cả, nên
  nó giữ đúng ngữ nghĩa "không có thứ tự nào được yêu cầu" mà `postgres::table_data` có khi nó bỏ
  hẳn mệnh đề `ORDER BY` (`postgres.rs:761`). Sắp theo cột đầu tiên thì ngược lại: một cột không
  index trên bảng lớn là một lần sort toàn bảng cho mỗi lần lật trang.
- `mssql_structure.rs`: `table_structure`/`table_stats`/`collations`, shape y hệt
  `postgres_structure.rs`. `table_stats` đọc từ `sys.dm_db_partition_stats`/`sys.partitions` (số
  dòng ước tính, giống MySQL/Postgres đọc từ catalog thay vì COUNT thật). `collations` đọc
  `sys.fn_helpcollations()`.
- `schema_outline`: một query tổng hợp cột của mọi bảng trong database, shape
  `SqlSchemaOutline`/`SqlOutlineTable`/`SqlOutlineColumn` không đổi.

**Frontend:**
- `src/modules/db/mssql/columns.ts`: `isAutoIncrement` (D7), `isGenerated` (cột computed —
  `sys.columns.is_computed`), `isServerAssigned` (`isAutoIncrement || isGenerated ||` cột
  `rowversion`/`timestamp` — xem D7), `isBinary` (`varbinary`/`binary`/`image`).
- `src/modules/db/mssql/system.ts`: `isMssqlSystemDatabase` — trả `true` cho
  `master/tempdb/model/msdb` dù chúng đã bị lọc khỏi `listDatabases` (D3-style: hàm vẫn nên tồn tại
  và đúng, phòng khi tương lai một chỗ khác gọi tới, giống cách Postgres vẫn định nghĩa dù
  `list_databases` cũng đã tự lọc trước).
- `src/modules/db/mssql/dialect.ts`: điền `kind: "mssql"`, `editing` tạm để rỗng/placeholder
  (Plan 6 điền thật), `regexpFilter: false` (D12), `cancellable: false` cho tới Plan 5, mọi cờ ghi
  (`writable`/`ddlWritable`/`rowsWritable`/`dumpRestoreWritable`) **để `false`** ở plan này — chỉ
  đọc, giống ClickHouse's giai đoạn đầu. `syntax` ở plan này dùng shape *cũ* của `SqlSyntax`
  (`identifierQuote` là một ký tự) và tạm để `null`: Query tab chưa mở nên chưa ai đọc tới nó, và
  Plan 4 mới đổi shape rồi điền `MSSQL_SYNTAX` thật.
- `engines.ts`: `SQL_ENGINES.mssql = { api: mssqlApi, dialect: mssqlDialect }`.

**Tiêu chí xong:** mở một bảng SQL Server có dữ liệu, thấy đúng cột/kiểu/khoá chính, phân trang và
lọc hoạt động, Structure tab hiện đúng cột + index (read-only), tất cả nút ghi (thêm dòng, sửa ô,
Add table...) đều khoá — giống trải nghiệm ClickHouse trước khi row-writes được mở.

### Plan 3 — Ghi dòng: insertRows / updateRow / deleteRows

**Phạm vi:** Data tab ghi được — bật `rowsWritable: true` trong `mssqlDialect`.

**Backend (`drivers/mssql.rs`):**
- `update_row`/`insert_rows`/`delete_rows`: copy khuôn Postgres nguyên xi (transaction, pre-check
  `matched = 1` trước UPDATE, `IS NOT DISTINCT FROM`-tương-đương). SQL Server không có
  `IS NOT DISTINCT FROM` chuẩn ANSI (dùng được từ SQL Server 2022 trở đi qua cú pháp khác, hoặc
  không có trên bản cũ hơn) — dùng mẫu tương thích ngược:
  `(col = @p OR (col IS NULL AND @p IS NULL))` cho mọi bản SQL Server, thay vì cược vào việc server
  test/server người dùng đủ mới.
- Không có khái niệm `(tableoid, ctid)` như Postgres cho bảng không khoá chính — SQL Server luôn có
  ít nhất `heap` với `%%physloc%%` (undocumented nhưng ổn định, dùng nội bộ bởi chính Microsoft's
  tooling) làm tương đương, hoặc đơn giản hơn: dùng toàn bộ cột làm key khi không có khoá chính
  (giống fallback `primaryKey.length > 0 ? primaryKey : columns` frontend đã tự làm — nghĩa là
  backend không cần cơ chế `%%physloc%%` phức tạp, chỉ cần WHERE đúng mọi cột và chấp nhận rủi ro
  trùng dòng y hệt MySQL đang chấp nhận, **không** cần bắt chước cơ chế đặc thù `ctid` của Postgres).
- Placeholder: `tiberius` dùng `@P1, @P2, ...` (named-ish nhưng đơn giản đếm số, giống spirit của
  Postgres's `$1, $2` — không phải `?` như MySQL).
- `DBCC CHECKIDENT ('schema.table', RESEED, 0)` cho reset counter, **có schema và chỉ gọi khi bảng
  thật sự có cột IDENTITY** (D7) — lệnh này báo lỗi trên bảng không có IDENTITY, khác
  `AUTO_INCREMENT = 1` của MySQL vốn vô hại, mà `deleteRows(all, resetAutoIncrement)` thì được gọi
  trên bảng bất kỳ.

**Frontend:** `mssqlDialect.rowsWritable = true`. Không đổi gì khác ở tầng UI — `SqlTable.tsx` đã
tổng quát hoá đủ qua `dialect`/`api`.

**Tiêu chí xong:** sửa một ô, thêm một dòng (kể cả bỏ trống cột có DEFAULT/IDENTITY), xoá dòng (kể
cả xoá hết + reset IDENTITY) trên bảng SQL Server test, y hệt trải nghiệm MySQL/Postgres.

### Plan 4 — Cú pháp: `SqlSyntax` thành cặp open/close, `MSSQL_SYNTAX`, tách batch `GO`

**Phạm vi:** thuần frontend, không lệnh backend mới, không tính năng người dùng thấy được. Đây là
plan dọn đường: sau nó, `splitStatements` và bộ lint hiểu `[bracket identifier]` và `GO`. **Phải
đứng trước Plan 5**, vì bộ tách câu chạy ở frontend và Query tab gọi thẳng nó — xem hộp trong D4.

- `sql/syntax.ts`: đổi `identifierQuote: string | null` thành
  `identifierQuote: { open: string; close: string } | null` (D4). Sửa ba hằng số hiện có —
  `MYSQL_SYNTAX`, `SQLITE_SYNTAX`, `CLICKHOUSE_SYNTAX` đều đang là `` "`" `` → 
  `` { open: "`", close: "`" } ``; `POSTGRES_SYNTAX` giữ `null`. Thêm `MSSQL_SYNTAX`:
  `identifierQuote: { open: "[", close: "]" }`, `hashComments: false`,
  `dashCommentNeedsSpace: false`, `nestedBlockComments: false` (SQL Server không nest `/* */`),
  `doubleQuoteIsIdentifier: true` (mặc định `QUOTED_IDENTIFIER ON`), `backslashEscapes: false`,
  `escapeStringPrefix: false`, `dollarQuoting: false`.
- Thêm field `batchSeparator: boolean` vào `SqlSyntax` (D9) — chỉ `true` cho MSSQL. Bộ tách đọc
  field này để tách thêm một tầng theo dòng chỉ chứa `GO` (hoặc `GO <n>`), **trước** khi tách theo
  `;` trong mỗi batch. Một chuỗi hay comment chứa chữ `go` không được nhầm là separator.
- Sửa cả **bốn** chỗ đọc `identifierQuote`, không phải một: `sql/statements.ts:126,131`,
  `sql/lint.ts:154,160`, và `sql/lint.ts:344` (`asWritten` — hàm này bọc tên bằng một ký tự và
  nhân đôi chính nó để escape; với `[`/`]` phải bọc bằng cặp và escape bằng nhân đôi ký tự đóng).
- Chạy lại **toàn bộ** `sql/syntax.test.ts`, `sql/statements.test.ts`, `sql/lint.test.ts` trước khi
  thêm test mới — đây là plan duy nhất đổi shape chạm các engine cũ, nên bằng chứng là test cũ vẫn
  xanh chứ không phải test mới xanh. `syntax.test.ts:21-22` đang assert giá trị cũ và phải sửa.
- `mssqlDialect.syntax = MSSQL_SYNTAX`.

**Tiêu chí xong:** test cũ của 4 engine xanh nguyên; `splitStatements` trên
`SELECT * FROM [Order;Details]; SELECT 1` trả về đúng **hai** câu (không phải ba), trên một script
có `GO`/`GO 3` trả về đúng số batch, và một comment `-- go` không cắt gì cả.

### Plan 5 — Query tab: run_script, cancelQuery, validateSql

**Phạm vi:** Query tab mở được, chạy multi-statement, hỗ trợ `GO` (D9), cancel được (D8),
validate cú pháp không chạy thật. **Chưa** bật `writable` (DDL/DML tay qua Query tab) — đó là
Plan 6, vì `guard.ts` cần biết `ddlWritable` trước khi cho phép DDL qua đường này.

**Backend (`drivers/mssql_script.rs` mới):**
- Tách batch theo `GO` (D9) trước khi tách câu theo `;` trong mỗi batch. Phía Rust **không** có
  `SqlSyntax` — `mysql_script.rs`/`postgres_script.rs` hard-code luật lexing riêng — nên đây là
  một splitter viết riêng, giữ đồng bộ với bản JS của Plan 4 bằng test song song (cùng input, cùng
  số câu) chứ không bằng một shape dùng chung.
- `run(pool, sql, on_session_id)`: mỗi batch một round trip, gom `StatementResult` mọi batch nối
  lại làm một danh sách, dừng toàn bộ script (mọi batch còn lại) nếu một câu lỗi — giữ đúng hợp đồng
  `SqlApi.runScript` hiện tại ("một statement lỗi dừng cả script, statement trước đó vẫn trả về
  kết quả").
- `verb`/`kind` (`rows`/`affected`/`ok`) suy từ statement text đầu batch, giống MySQL/Postgres.
- Session id cho cancel: `SELECT @@SPID` đầu phiên (giống `thread_id`/`CONNECTION_ID()` MySQL).
- `cancel(pool, session_id)`: xem D8 — code lúc này mới xác nhận `tiberius` có Attention API hay
  phải rơi về `KILL`. Nếu rơi về `KILL`, phải chốt luôn cách xử lý quyền `ALTER ANY CONNECTION`
  theo D8 và **thử với một login không phải `sa`** trước khi coi là xong; test bằng `sa` không
  chứng minh được gì ở đây.
- `validate`: SQL Server không có cách "parse mà không chạy" rẻ như MySQL's `PREPARE`/Postgres's
  `PREPARE` — tương đương gần nhất là `SET PARSEONLY ON; <statement>; SET PARSEONLY OFF;` (server
  parse cú pháp, không thực thi, không cả resolve tên bảng/cột — nghĩa là *ít* warning hữu ích hơn
  Postgres's `PREPARE`, chỉ bắt lỗi cú pháp thô, gần giống mức "chỉ syntax error" MySQL đang có).
  Ghi rõ giới hạn này trong doc comment của `mssqlApi.validateSql`, đừng hứa quá tay.

**Frontend:** không đổi `SqlApi` shape, không đổi UI — Query tab đã tổng quát hoá qua `dialect`.

**Tiêu chí xong:** dán một script T-SQL nhiều batch ngăn bởi `GO` (kể cả `GO 3` lặp batch, kể cả
comment `-- go` không bị nhầm là separator) vào Query tab, chạy ra đúng kết quả từng câu; bấm Cancel
giữa chừng một câu chạy lâu (`WAITFOR DELAY`) dừng được.

### Plan 6 — DDL: database/table/column/index

**Phạm vi:** Structure tab ghi được, sidebar "Add table"/Drop database hoạt động. Bật
`ddlWritable: true`, `writable: true` (Query tab giờ nhận DDL/DML tay gõ).

**Backend (`drivers/mssql_ddl.rs` mới), copy khuôn `postgres_ddl.rs`:**
- `create_database`: `CREATE DATABASE [name]`, **kèm `COLLATE <name>` khi tham số `collation` khác
  `null`** — `SqlApi.createDatabase(id, name, collation)` (`sql/api.ts:137`) đã có sẵn tham số đó
  và D14 mở ô nhập cho nó.
- `drop_database`: `DROP DATABASE [name]` — SQL Server từ chối nếu có kết nối khác đang mở
  database đó, giống PostgreSQL. `USE master` trên một connection là **không đủ**: pool có nhiều
  connection và mỗi cái giữ database context riêng, nên connection nào đang đứng trên database bị
  drop vẫn chặn lệnh. Cách chắc chắn là `ALTER DATABASE [name] SET SINGLE_USER WITH ROLLBACK
  IMMEDIATE` rồi `DROP DATABASE [name]`, hoặc đóng và dựng lại pool. Đây **không** đơn giản hơn
  `Pools::close_pool` của Postgres như bản nháp trước của spec này nói — nó chỉ khác kiểu.
- `create_table`: một bảng rỗng với cột `id INT IDENTITY(1,1) PRIMARY KEY`, giống mẫu MySQL/
  Postgres tạo lúc bấm "Add table".
- `rename_table`: `EXEC sp_rename 'schema.old', 'new'` (không có `ALTER TABLE ... RENAME TO` chuẩn
  ANSI trên SQL Server — `sp_rename` là thủ tục hệ thống, cách duy nhất). Hai lưu ý: tham số là
  **chuỗi**, không phải định danh `[ ]`, và tên mới **không** mang schema (nó không đổi được
  schema); `sp_rename` trả về một *warning message* về việc script cũ có thể gãy — `run` không được
  hiểu nhầm message đó là lỗi.
- `add_column`: `ALTER TABLE ... ADD` — thẳng, giống hai engine kia.
- `modify_column`/`drop_column`: **không phải một câu lệnh, xem D15.** `modify_column` là một chuỗi
  chạy trong một transaction: đọc default constraint (`sys.default_constraints`) và index
  (`sys.indexes`/`sys.index_columns`) đang gắn vào cột → drop những cái chặn `ALTER COLUMN` →
  `sp_rename 'schema.table.old', 'new', 'COLUMN'` nếu tên đổi → `ALTER COLUMN` với định nghĩa
  **đầy đủ** dựng lại từ `SqlColumnSpec` (kiểu, nullable, collation — kể cả phần người dùng không
  sửa, vì `ALTER COLUMN` thay thế toàn bộ định nghĩa và im lặng biến cột thành nullable nếu thiếu
  `NOT NULL`) → dựng lại default constraint và index đã drop. `drop_column` phải drop default
  constraint của cột trước khi drop cột. Đổi IDENTITY on/off là phi mục tiêu (D15) — Structure tab
  khoá ô đó với thông báo rõ thay vì để người dùng bấm rồi nhận lỗi server.
- `add_index`/`modify_index`/`drop_index`: `CREATE [UNIQUE] [CLUSTERED|NONCLUSTERED] INDEX`.
  Primary key không phải "một loại index tạo bằng CREATE INDEX" như MySQL/Postgres — nó là một
  constraint (`ALTER TABLE ... ADD CONSTRAINT ... PRIMARY KEY`) tuy về mặt vật lý vẫn là index —
  `SqlEditing.primaryKeyName` (đã có sẵn field cho đúng trường hợp này, xem `sql/dialect.ts:59`)
  dùng để cho phép đặt tên constraint thay vì khoá cứng như MySQL.

**Frontend:**
- `src/modules/db/mssql/editing.ts`: điền `SqlEditing` thật — `columnTypes` (bảng kiểu T-SQL:
  `int`, `bigint`, `smallint`, `tinyint`, `bit`, `decimal(p,s)`, `float`, `real`, `money`,
  `smallmoney`, `varchar(n)`, `nvarchar(n)`, `char(n)`, `nchar(n)`, `text`/`ntext` (deprecated,
  vẫn liệt kê vì DB cũ còn dùng), `date`, `time`, `datetime`, `datetime2`, `smalldatetime`,
  `datetimeoffset`, `varbinary(n)`, `binary(n)`, `uniqueidentifier`, `xml`), `unsigned: false`
  (không có UNSIGNED), `columnPosition: false` (không có `FIRST`/`AFTER`, luôn append — giống
  Postgres), `onUpdateCurrentTimestamp: false` (không có, tương đương là trigger), `autoIncrement:
  true` (D7), `databaseCollation: true` /
  `tableCollation: false` (**D14** — có collation cấp database, không có cấp bảng; plan này tách
  `objectCollation` cũ thành hai field và cập nhật bốn engine kia theo, hành vi của chúng không
  đổi), `markExpressionDefaults:
  true`, `indexKinds`: `["primary", "unique", "index"]` (không có fulltext/spatial trong v1 —
  SQL Server có cả hai nhưng cú pháp riêng biệt hẳn, để lại phi mục tiêu), `indexMethods`:
  `["CLUSTERED", "NONCLUSTERED"]` (khái niệm khác hẳn MySQL's BTREE/HASH — đây là "có sắp xếp vật
  lý theo index này hay không", field có sẵn dùng vừa vặn dù ý nghĩa khác), `indexPrefix: false`,
  `primaryKeyName: null` (đặt tên tự do, giống Postgres).
- `mssqlDialect`: `ddlWritable = true`, `writable = true`, `dumpRestoreWritable` vẫn `false` tới
  Plan 7.

**Tiêu chí xong:** trên database test, tạo bảng mới từ sidebar, thêm/sửa/xoá cột và index qua
Structure tab, đổi tên bảng, xoá bảng/database — mọi thao tác phản ánh đúng qua SSMS hoặc
`sqlcmd` chạy tay kiểm lại.

### Plan 7 — Dump & restore

**Phạm vi:** `DatabaseActions`'s Dump/Restore hoạt động. Bật `dumpRestoreWritable: true`.

- Chốt lại D10 trước khi code: thử `sqlcmd` (Go, mã nguồn mở,
  <https://github.com/microsoft/go-sqlcmd>) có tải/pin version được theo đúng khuôn `tools.rs`
  (`Suite::Mssql`, checksum pin cứng) hay không — nếu được, dùng nó cho **restore** (thay `psql`/
  `mysql` client). Nếu tải được cả bản build sẵn ổn định trên cả ba platform, cân nhắc dùng luôn cho
  cả liệt kê nhưng **không** cho dump (vẫn không giải quyết việc sinh DDL+INSERT thành file).
- **Dump: viết tự thân, không tool ngoài**, lấy `clickhouse_dump.rs` làm khuôn (D10). Tái dùng
  `mssql_structure::table_structure` (Plan 2) để sinh `CREATE TABLE`+`CREATE INDEX` mỗi bảng, và
  `mssql::table_data` (Plan 2, đọc theo trang thay vì `SELECT *` một lần — bảng lớn không load hết
  vào RAM) để sinh câu `INSERT` hàng loạt.

  **Ba ràng buộc của T-SQL mà thiếu cái nào thì tiêu chí "dump → restore → dữ liệu khớp" cũng
  không đạt:**
  - **`SET IDENTITY_INSERT [schema].[table] ON` bao quanh phần INSERT của mọi bảng có cột
    IDENTITY**, và `OFF` ngay sau. Không có nó, server tự đánh số lại khoá chính khi restore và mọi
    khoá ngoại trỏ vào bảng đó thành rác — dữ liệu "khớp" theo số dòng nhưng sai theo quan hệ. Lưu
    ý SQL Server chỉ cho **một** bảng bật `IDENTITY_INSERT` tại một thời điểm trong một session,
    nên phải bật/tắt theo từng bảng chứ không bật một lượt đầu file.
  - **Thứ tự khoá ngoại.** Chọn cách bền hơn là sắp topological: sinh `CREATE TABLE` **không kèm
    FOREIGN KEY** → INSERT hết mọi bảng → `ALTER TABLE ... ADD CONSTRAINT ... FOREIGN KEY` gom ở
    cuối file. Một chu trình FK (bảng tự tham chiếu hay hai bảng trỏ nhau) làm topological sort bế
    tắc, còn cách này thì không.
  - **Kích thước lô INSERT:** `INSERT ... VALUES` của SQL Server tối đa **1000 dòng một câu**, và
    một request tối đa **2100 tham số**. Lô phải lấy `min(1000, 2100 / số_cột)` chứ không phải một
    hằng số "đủ nhỏ" đoán bằng cảm tính.

  `mode: "structure" | "data" | "all"` giữ nguyên nghĩa. Tiến độ báo qua
  `TRANSFER_PROGRESS_EVENT` như các driver khác — tính theo số bảng đã sinh xong / tổng số bảng
  (không có ước lượng `data_size` đẹp như `pg_dump`/`mysqldump` báo, vì không có tool ngoài đếm hộ
  — chấp nhận progress bar "chạy nhưng không có số phần trăm chính xác", giống trường hợp
  `data_size` không đọc được đã có sẵn trong `dump.rs::Watch`).
- **Restore:** nếu `sqlcmd` khả dụng (theo D10), chạy `sqlcmd -S host,port -U user -P password -d
  database -i path`. Nếu quyết định không đưa `sqlcmd` vào, restore cũng tự thân: đọc file `.sql`,
  tách batch theo `GO` (dùng lại bộ tách Plan 5), chạy tuần tự qua `mssql_script::run`.

**Tiêu chí xong:** Dump một database ra file, tạo database rỗng mới, Restore file đó vào, so sánh
dữ liệu khớp — vòng lặp dump→restore giữ nguyên dữ liệu, giống test đã có cho MySQL/Postgres.

### Plan 8 — Hoàn thiện: CodeMirror, reserved words, connection form, i18n, CHANGELOG

**Phạm vi:** Query tab tô màu đúng T-SQL, autocomplete/lint gợi ý đúng, connection form hoàn thiện,
tài liệu cập nhật. Phần `SqlSyntax`/bộ tách câu đã xong ở Plan 4, nên plan này không còn đổi shape
nào chạm các engine cũ.

- `src/modules/db/components/SqlEditor/extensions.ts`/`theme.ts`: đăng ký `sql({ dialect: MSSQL,
  ...KHÔNG cần override identifierQuotes vì MSSQL dialect của CodeMirror đã tự có `[` sẵn — chỉ
  cần dùng đúng export `MSSQL` từ `@codemirror/lang-sql` như đã xác nhận có sẵn }))`.
- `sql/lint.ts::reservedWords`: đã tổng quát (đọc từ `cmDialect`), không cần việc riêng — chỉ cần
  `mssqlDialect.cmDialect = MSSQL` là đủ.
- Connection form: hoàn thiện label/placeholder tiếng Việt/Anh (`i18n/vi.ts`, `i18n/en.ts`),
  `connectionForm.test.ts` thêm case cho `mssql` giống các kind khác.
- `CHANGELOG.md`: một dòng ngắn theo convention hiện có (`.agent/conventions/changelog.md`) khi
  Plan 1 merge, không dồn hết tới cuối — mỗi plan xong tự thêm dòng của mình, giống cách ClickHouse
  được ghi nhận tăng dần qua nhiều spec.

**Tiêu chí xong:** gõ T-SQL trong Query tab, `[Order Details]`/comment `--`/`/* */` tô màu đúng,
autocomplete gợi ý tên bảng/cột đúng, chạy lại toàn bộ test suite hiện có (không riêng MSSQL) xanh.

## Việc để ngỏ, cần chốt trong lúc code (không phải thiếu sót của spec — phụ thuộc thử với server thật)

Mấy điểm này nằm ở lớp "chi tiết triển khai phụ thuộc crate/tool đang ở version nào tại thời điểm
code", không đổi kiến trúc tổng thể nếu câu trả lời đi khác dự đoán. Câu trả lời quay về đây khi
plan tương ứng xong.

### Đã chốt

- **D2 — pool crate: `deadpool` 0.12.3 trần**, không phải `deadpool-tiberius`. `Manager` của
  deadpool 0.12 chỉ đòi `create` + `recycle`, nên không đáng thêm một crate cầu nối. `max_size = 8`,
  `recycle` chạy `SELECT 1`. (Plan 1)
- **D6 — `trust_cert()` luôn bật**, không thêm ô "Trust server certificate" riêng. Kèm theo một
  đính chính: `EncryptionLevel::Off` của `tiberius` nghĩa là *chỉ mã hoá lúc đăng nhập*, không phải
  *không mã hoá* — cái đó là `NotSupported`. Xem bảng trong D6. (Plan 1)
- **D5 — ba chỗ đăng ký kind mà checklist ban đầu bỏ sót** (`icons.tsx::BRAND_MARKS`, union `kind`
  riêng của `DatabaseActions`/`DatabaseStats`, và vế `ToolSuite | null`). Đã ghi vào D5. (Plan 1)

### Còn ngỏ

- **D8** — `tiberius` có Attention/cancel API cấp-statement hay phải rơi về `KILL` cấp-session, và
  nếu là `KILL` thì xử lý quyền `ALTER ANY CONNECTION` theo cách nào (đo lúc connect, hay để lỗi
  nổi lên UI). Chốt ở **Plan 5**, và phải thử bằng một login **không phải `sa`**.
- **D10** — `sqlcmd` (Go) có đưa vào `tools.rs` được không, hay restore cũng tự thân luôn. Chốt ở
  **Plan 7**.
- **D13** — con số `LOCK_TIMEOUT` cụ thể. Plan 1 đặt **5000 ms** trong `mssql::dial`, nhưng đó là
  con số chọn trên giấy: server test tắt trước khi kiểm được, nên nó **chưa được đo với một bảng
  đang thật sự bị khoá**. Còn ngỏ cho tới khi có phiên đo đó.

### Chưa được xác minh với server thật (Plan 1)

Server test `192.168.50.86:1433` tắt giữa chừng lúc chạy Plan 1 — cùng host, các cổng 3307/3308/
5432/4306/27017 vẫn mở, nên là instance chứ không phải mạng. Những thứ sau **đã viết nhưng chưa
chạy lần nào với server thật**, và là việc đầu tiên phải làm khi server lên lại:

- `connect` qua `tiberius` + `trust_cert` với certificate tự ký.
- `server_info` cắt `SERVERPROPERTY` + phần OS của `@@VERSION`.
- `list_databases` với ba điều kiện `database_id > 4` / `state = 0` / `HAS_DBACCESS`.
- `list_tables` đọc `sys.objects` — đặc biệt là **view có xuất hiện không** và thứ tự `dbo` trước.
- Đường SSH tunnel.
- `SET LOCK_TIMEOUT` có được server nhận trên connection mới không.

## Thứ tự plan, tóm tắt

| Plan | Nội dung | Cờ bật sau khi xong |
| --- | --- | --- |
| 1 | Khung kết nối, server info, list databases/tables | — (chưa vào `SQL_ENGINES`) |
| 2 | Đọc bảng: table data, structure, outline, collations | vào `SQL_ENGINES`, mọi cờ ghi `false` |
| 3 | Ghi dòng | `rowsWritable` |
| 4 | `SqlSyntax` open/close + `MSSQL_SYNTAX` + tách `GO` (D4/D9) | — (dọn đường, không tính năng) |
| 5 | Query tab: run_script, cancel, validate | `cancellable` |
| 6 | DDL: database/table/column/index (D14/D15) | `ddlWritable`, `writable` |
| 7 | Dump & restore | `dumpRestoreWritable` |
| 8 | CodeMirror, i18n, connection form, CHANGELOG | — |

Hai plan đổi shape chạm các engine cũ là **4** (`SqlSyntax.identifierQuote`) và **6**
(`SqlEditing.objectCollation` tách đôi). Cả hai đều phải chạy lại toàn bộ test hiện có, không chỉ
test mới của MSSQL.
