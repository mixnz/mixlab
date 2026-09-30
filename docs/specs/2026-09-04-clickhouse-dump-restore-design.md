---
status: implemented
date: 2026-09-04
---

# ClickHouse: dump/restore

Ngày: 2026-09-04

## Mục tiêu

D10 của plan v1 (`2026-09-04-clickhouse-db-kind.md`, gitignored) và mục "Những
gì để lại" của spec row-writes vẫn còn nguyên: ClickHouse chưa có dump/restore. Spec này mở nốt mảng
cuối trong ba mảng CHANGELOG từng ghi nợ — sau **editing** (row-writes) và **DDL** (ddl-design), giờ
là **dump/restore**.

Sau khi làm xong: nút Dump trên sidebar (đã có UI dùng chung với MySQL/Postgres/SQLite qua
`DatabaseActions`/`DumpDialog`) mở được cho ClickHouse, cho chọn 1 trong 3 mode (structure/data/all),
ghi ra một file `.sql`. Nút Restore đọc lại file đó, chạy vào database đang chọn — kể cả một database
khác tên với database gốc lúc dump. Cả hai có progress bar + Cancel, giống ba engine kia.

## Phi mục tiêu

- **Không cần cài `clickhouse-client` hay bất kỳ tool ngoài nào.** Toàn bộ đi qua HTTP interface đã
  có trong `clickhouse.rs` — không có tool-discovery/download như `mysqldump`/`pg_dump`/`mongodump`.
- **Query tab vẫn đóng** với DDL gõ tay ngoài 4 verb DML đã mở (D8 dưới đây tách hẳn cờ dump/restore
  ra khỏi `writable`, không đụng gì đến Query tab).
- **Không có `CREATE DATABASE` trong file dump** — giống cả 3 engine kia, file dump không tự tạo
  database, restore vào bất kỳ database nào user đang chọn.
- **Không dump `CREATE DICTIONARY`.** Dictionary là một loại DDL object khác `CREATE TABLE`, và
  `system.tables`/`system.columns` — nguồn dữ liệu app đang đọc cho mọi thứ khác — không phủ đủ nó.
  Ghi nhận là giới hạn đã biết, không phải bỏ sót.
- **Không có `KILL QUERY`/`KILL MUTATION` thật cho Cancel** — xem D6.
- **Không giới hạn kích thước file** — giống 3 engine kia, không thêm cảnh báo/giới hạn dung lượng.

## Hiện trạng

| Chỗ | Điều spec dựa vào |
| --- | --- |
| [`src-tauri/src/modules/db/drivers/clickhouse.rs`](../../apps/desktop/src-tauri/src/modules/db/drivers/clickhouse.rs) | `Connection`, `query_in_database` (đã xác nhận `?database=` scope một tên bảng không qualify y hệt `USE`), `execute_check`, `quote_ident`/`qualified`, `table_columns` |
| [`src-tauri/src/modules/db/drivers/clickhouse_script.rs`](../../apps/desktop/src-tauri/src/modules/db/drivers/clickhouse_script.rs) | `split_statements` — bộ quét ký tự nhận diện quote/comment của ClickHouse, hiện chạy trên `Vec<char>` gom hết vào RAM một lần; cần tách phần lõi ra dùng lại được theo kiểu incremental (D7) |
| [`src-tauri/src/modules/db/drivers/dump.rs`](../../apps/desktop/src-tauri/src/modules/db/drivers/dump.rs) | `DumpMode` (structure/data/all), `Progress`, `Watch` (đều `pub`), `Tracker` (hiện `struct` riêng của module, cần nới tầm nhìn) — thuật toán ước lượng % dựa trên trọng số byte mỗi bảng + kích thước file đang ghi, đúng thứ tái dùng được cho HTTP streaming |
| [`src-tauri/src/modules/db/drivers/sqlite_dump.rs`](../../apps/desktop/src-tauri/src/modules/db/drivers/sqlite_dump.rs) | Mẫu module dump không cần child process: đọc `CREATE` sẵn có từ hệ thống, ghi thẳng ra file, restore chạy qua script runner của chính engine đó |
| [`src-tauri/src/modules/db/commands/postgres.rs:355-430`](../../apps/desktop/src-tauri/src/modules/db/commands/postgres.rs) | Mẫu wiring command: `Transfer::start`/`.flag()`, `reporter()`, `dump::Watch` — không dùng `in_background` cho ClickHouse vì không có child process chặn luồng |
| [`src-tauri/src/modules/db/commands/clickhouse.rs`](../../apps/desktop/src-tauri/src/modules/db/commands/clickhouse.rs) | Không có `in_background`/`tools::require` nào — mọi lệnh ClickHouse hiện chạy async thẳng |
| [`src/modules/db/clickhouse/api.ts:147-148`](../../apps/desktop/src/modules/db/clickhouse/api.ts) | `dump`/`restore` hiện `notSupported()` |
| [`src/modules/db/clickhouse/dialect.ts:41`](../../apps/desktop/src/modules/db/clickhouse/dialect.ts) | `writable: false` — comment hiện ghi cờ này gác luôn dump/restore, cần tách |
| [`src/modules/db/sql/dialect.ts:127-149`](../../apps/desktop/src/modules/db/sql/dialect.ts) | `writable`'s doc: "The Query tab may send writing statements, **and the database as a whole may be dumped and restored**" — câu này cần sửa khi tách D8 |
| [`src/modules/db/components/DatabaseActions/DatabaseActions.tsx:63-74,142-150`](../../apps/desktop/src/modules/db/components/DatabaseActions/DatabaseActions.tsx) | `suite = null` cho `clickhouse` (đúng, giữ nguyên — không có tool để tải); comment dòng 70-73 nói sai sau spec này ("v1 has no dump or restore... at all"), cần sửa; `modes` prop dùng `undefined` cho mọi kind trừ sqlite — ClickHouse tự động rơi vào nhánh "cả 3 mode", không cần đổi dòng đó |
| [`src/modules/db/sql/SqlWorkspace.tsx:686-700`](../../apps/desktop/src/modules/db/sql/SqlWorkspace.tsx) | `<DatabaseActions disabled={tablesLoading \|\| readOnly} schemaDisabled={tablesLoading \|\| schemaReadOnly} .../>` — `disabled` (không phải `schemaDisabled`) chính là cờ gác dump/restore, hiện fold thẳng theo `readOnly` (tức `writable`) |
| [`src/modules/db/DbTab.tsx:767`](../../apps/desktop/src/modules/db/DbTab.tsx) | Nơi duy nhất tính `readOnly` cho `SqlWorkspace` từ `dialect.writable` |
| `docs/specs/2026-09-04-clickhouse-row-writes-design.md`'s D6 | Tiền lệ y hệt: tách `rowsWritable` khỏi `writable` bằng cách thêm field + prop `dataReadOnly` xuyên suốt `DbTab → SqlWorkspace → SqlTable`. D8 dưới đây lặp lại đúng mẫu này cho dump/restore |

## Quyết định đã chốt

**D1 — HTTP-only, không tool ngoài.**
Toàn bộ dump/restore đi qua `reqwest`/HTTP interface đã có, giống cách mọi thứ khác của ClickHouse
trong app đang làm — không có `clickhouse-client` để tìm/tải như D10 của plan v1 từng cân nhắc rồi
loại bỏ. Structure lấy từ `SHOW CREATE TABLE`; data lấy từ `SELECT * FROM t FORMAT SQLInsert` — cả
hai đã là SQL hợp lệ do chính server sinh ra, không cần tự viết literal encoder như SQLite từng phải
hoãn.

**D2 — Cả 3 mode: structure / data / all.**
`DumpDialog` không cần đổi (`modes={undefined}` → cả 3 tuỳ chọn), khớp MySQL/Postgres. File "all"
ghi structure trước (mọi bảng/view), rồi data sau (INSERT của các bảng có dữ liệu thật) — giống bố
cục pg_dump plain-format, không xen kẽ CREATE/INSERT theo từng bảng như mysqldump, vì ClickHouse
không có FK nên thứ tự xen kẽ hay tách khối đều an toàn như nhau; tách khối đơn giản hơn để viết.

**D3 — Restore: `DROP TABLE IF EXISTS` trước mỗi `CREATE TABLE`.**
Giống mysqldump. File dump luôn ghi cặp `DROP TABLE IF EXISTS \`table\`; CREATE TABLE ...;` cho mỗi
bảng/view — restore luôn thành công, ghi đè sạch. Rủi ro mất dữ liệu khi restore nhầm database đã có
dữ liệu thật được chấp nhận (đã chốt ở vòng hỏi trước) — ghi lại ở mục Rủi ro.

**D4 — Bóc tên database: strip về dạng không qualify ngay lúc dump, không phải đổi tên lúc restore.**

Sửa lại so với cách trình bày ban đầu: **không** "đổi origin_db → target_db lúc restore" (lúc đó
không còn biết origin_db là gì để mà thay), mà **bóc hẳn tiền tố database ra khỏi mọi statement ngay
khi ghi file dump** — vì lúc dump, "database nào đang được bóc" là dữ kiện đã biết chắc (chính là
`database` truyền vào `dump_structure`). Statement đã bóc hết tiền tố (`CREATE TABLE table (...)` chứ
không phải `CREATE TABLE db.table (...)`) không còn cần biết "restore vào database nào" ở bước ghi
file — việc đó dồn hết vào lúc chạy: `execute_check`/`query_in_database` đều nhận `database:
Option<&str>` và scope qua `?database=` (đã xác nhận hoạt động đúng như `USE`, xem Hiện trạng), nên
statement đã unqualify tự resolve vào bất kỳ database đích nào tham số đó trỏ tới, không cần code nào
"biết" tên gốc nữa.

Bộ quét (tách từ lõi ký tự của `split_statements` — cùng cách nhận diện quote/comment, xem D7) duyệt
toàn bộ text mỗi statement, mọi vị trí identifier (backtick hoặc trần) đứng ngay trước `.` và khớp
đúng tên database đang dump → xoá cả identifier lẫn dấu `.`. Bắt được:
- `CREATE TABLE db.table (...)` — tiền tố đầu câu.
- `CREATE VIEW db.v AS SELECT ... FROM db.t` — tham chiếu trong `AS SELECT`.
- `CREATE MATERIALIZED VIEW db.mv TO db.target (...) AS SELECT ... FROM db.source` — cả tên MV, `TO`,
  lẫn `AS SELECT`.

**Ngoại lệ có chủ đích, không phải thiếu sót: `ENGINE = Distributed('cluster', 'db', 'table', ...)`.**
Tên database ở đây là **string literal**, một tham số dữ liệu của engine, không phải một vị trí tham
chiếu định danh được `?database=` resolve — không có khái niệm "unqualify" cho nó. Distributed table
vốn dĩ là proxy trỏ tới dữ liệu nằm ở nơi khác (có thể trên cluster khác hẳn); giữ nguyên tham số này
là đúng về ngữ nghĩa (proxy vẫn trỏ đúng chỗ dữ liệu thật sau khi restore), không rewrite. Chỉ tiền tố
tên bảng ở đầu câu (`CREATE TABLE db.dist_table`) được bóc như mọi bảng khác — bản thân object DDL
vẫn được tạo đúng vào database đích, chỉ có nó *trỏ đi đâu* là giữ nguyên.

`CREATE DICTIONARY` nằm ngoài phạm vi (xem Phi mục tiêu).

**D5 — Data dump: stream thẳng response ra file, không buffer JSON.**
`query_with_params`/`query()` hiện có gom hết response vào String rồi `serde_json::from_str` — phù
hợp cho việc đọc lưới (đã giới hạn `LIMIT`/`page_size`) nhưng không phù hợp cho export cả bảng. Viết
hàm mới trong `clickhouse_dump.rs` dùng `reqwest::Response::bytes_stream()`, ghi từng chunk thẳng vào
`File` đang mở bằng `tokio::io::AsyncWriteExt::write_all` (hoặc `std::io::Write` nếu chạy trong
`spawn_blocking` — xem D6 về việc có cần blocking hay không), không giữ toàn bộ response trong RAM.

**D6 — Progress + Cancel đầy đủ, tái dùng `Progress`/`Watch`/`Tracker` của `dump.rs`.**
`Tracker` hiện là `struct` riêng (không `pub`) — nới thành `pub(super)` (dùng được từ
`clickhouse_dump.rs`, cùng cấp `drivers::`). Thuật toán của nó (trọng số byte mỗi bảng từ
`table_stats().total_bytes`, nội suy theo kích thước file đang lớn dần, hiệu chỉnh tỉ lệ khi có bảng
đủ lớn) khớp thẳng với việc mọi bảng được ghi nối tiếp vào **một** file — không cần viết lại, chỉ cần
gọi `tracker.reached(table_name)` ngay trước khi bắt đầu stream/ghi CREATE của từng bảng, và
`tracker.progress()` sau mỗi chunk ghi được, y hệt cách `mysql_dump`/`postgres_dump` dùng nó qua
dòng stderr — chỉ khác nguồn tín hiệu "đã sang bảng mới" là vòng lặp tự gọi, không phải parse output
tool.

Cancel: `watch.cancel()` được poll giữa mỗi chunk stream (dump) và giữa mỗi statement (restore) —
đúng hạt giống ở đây là "table/statement", không phải "dòng stderr" như 3 engine kia.

**Giới hạn phải ghi rõ, không phải bug:** dừng client-side không đảm bảo ClickHouse server dừng ngay
câu `SELECT`/`INSERT` đang chạy — thiếu `query_id` tracking + `KILL QUERY` (đúng hạn chế đã ghi ở
`dialect.cancellable: false`, D4 của spec row-writes cũng gặp câu chuyện tương tự với mutation).
ClickHouse *thường* tự phát hiện socket đứt và dừng, nhưng chưa được verify bằng server thật — việc
verify này là một bước sớm khi thực thi plan, không phải quyết định thiết kế.

Không dùng `in_background`/`spawn_blocking`: không có child process nào chặn luồng — toàn bộ chạy
async thẳng trong tauri command, như mọi lệnh ClickHouse khác hiện có.

**D7 — Restore: đọc/tách statement kiểu incremental, chia sẻ lõi quét ký tự với `split_statements`.**
`split_statements` hiện gom cả script vào `Vec<char>` rồi duyệt bằng vòng lặp có "nhìn trước"
(`chars.get(i+1)`, `chars.get(i)` sau khi đóng quote). Để đọc theo chunk từ `BufReader` mà không cần
tải hết file dump vào RAM, lõi quét cần chuyển từ "vòng lặp có nhìn trước trên slice đầy đủ" sang một
**state machine resumable**: mỗi lần gọi chỉ xử lý phần buffer hiện có, dừng lại ở giữa chừng nếu hết
buffer trong lúc còn đang ở giữa quote/comment, và tiếp tục đúng chỗ khi buffer được nạp thêm. Trạng
thái cần giữ giữa các lần gọi: đang ở ngoài mọi vùng đặc biệt / trong line-comment / trong block-
comment (kèm độ sâu) / trong backtick-hoặc-double-quote / trong single-quote — cộng cờ "vừa gặp `\`,
ký tự sau là literal" và cờ "vừa đóng `'`, chờ xem ký tự kế có phải `'` thứ hai không" (doubled-quote
escape cần nhìn trước 1 ký tự, có thể rơi đúng ranh giới chunk).

Cả `split_statements` (dùng cho Query tab, vẫn nhận toàn bộ text một lần) và splitter mới (dùng cho
restore, nhận từng chunk) gọi chung một hàm chuyển trạng thái cấp ký tự — tránh lặp lại/lệch nhau
đúng tinh thần cảnh báo sẵn có trong doc của `split_statements` ("a change to either splitter belongs
in the same commit as the other").

Vì một statement vẫn phải nguyên vẹn khi gửi lên server (không gửi được nửa câu `INSERT`), bộ nhớ
đỉnh khi restore chỉ còn bị chặn bởi kích thước **một statement lớn nhất** trong file — với data dump
từ `FORMAT SQLInsert` (mặc định gộp nhiều dòng/statement theo
`output_format_sql_insert_max_batch_size`), đó là một batch, không phải cả bảng. Cần verify con số
batch mặc định thật trên server test trước khi chốt plan (ghi ở Rủi ro).

Mỗi statement tách được chạy qua `execute_check` (không phải `clickhouse_script::run` — hàm đó không
có callback progress/cancel và trả về kết quả dạng `Vec<StatementResult>` dành cho Query tab hiển
thị, thừa cho restore). Dừng ở statement đầu tiên lỗi, báo statement đó + lỗi server — giống
`sqlite_dump::restore`.

**D8 — `dumpRestoreWritable` — field mới trên `SqlDialect`, tách khỏi `writable`.**
Lặp lại đúng mẫu D6 của spec row-writes (khi đó tách `rowsWritable` ra khỏi `writable`):

- `SqlDialect` thêm `dumpRestoreWritable: boolean`. MySQL/Postgres/SQLite: `true`. ClickHouse: `true`
  — thay đổi duy nhất của phase này ở tầng dialect. `writable` giữ nguyên `false` cho ClickHouse,
  doc comment của nó sửa lại: bỏ vế "and the database as a whole may be dumped and restored".
- `SqlWorkspaceProps` thêm `dumpRestoreReadOnly?: boolean` (default `false`), doc giống mẫu
  `dataReadOnly`/`schemaReadOnly` đã có.
- `DbTab.tsx` tính `dumpRestoreReadOnly={(activeSavedConnection?.readOnly ?? false) ||
  !engine.dialect.dumpRestoreWritable}`, truyền vào `SqlWorkspace` cùng chỗ với `readOnly`.
- `SqlWorkspace.tsx`: `<DatabaseActions disabled={tablesLoading || dumpRestoreReadOnly} .../>` thay
  cho `disabled={tablesLoading || readOnly}` hiện tại (dòng 695) — `readOnly` (bare) chỉ còn ảnh
  hưởng `QueryEditor` (dòng 783), đúng nghĩa hẹp lại của nó.

**D9 — Data dump bỏ qua engine không phải kho dữ liệu thật.**
Trước khi export data một bảng, đọc `system.tables.engine` (đã có sẵn qua `table_engine` trong
`clickhouse.rs`) và bỏ qua nếu thuộc danh sách loại trừ: `View`, `MaterializedView` (không có
storage riêng — xem D4), `Distributed`, `Kafka`, `RabbitMQ`, `NATS`, `FileLog`, `MySQL`,
`PostgreSQL`, `S3`, `URL`, `HDFS`, `ODBC`, `JDBC`, `ExternalDistributed`. Danh sách loại trừ (không
phải danh sách cho phép) — bảng thuộc engine không nằm trong danh sách này (kể cả engine lạ mai sau
ClickHouse thêm) vẫn được thử dump data, để server tự báo lỗi nếu không hợp lệ thay vì âm thầm bỏ
qua một engine hợp lệ nhưng app chưa biết tới. Structure (CREATE TABLE) vẫn dump cho mọi engine,
không lọc — giữ định nghĩa kết nối/proxy để restore vẫn tạo lại đúng object đó.

## Backend — file đổi

```
src-tauri/src/modules/db/drivers/clickhouse_script.rs
  fn split_statements  → tách lõi state machine cấp ký tự thành hàm/dạng dùng chung, pub(super)
                          để clickhouse_dump.rs gọi được cho splitter incremental (D7)

src-tauri/src/modules/db/drivers/dump.rs
  struct Tracker  → pub(super) (D6)

src-tauri/src/modules/db/drivers/clickhouse_dump.rs   (module mới)
  + pub async fn dump_structure(conn, database, path, mode: dump::DumpMode, watch: &dump::Watch)
      viết DROP+CREATE (mode Structure/All) cho mọi bảng/view — D3, D4
  + pub async fn dump_data(conn, database, path, watch: &dump::Watch)  (mode Data/All, nối tiếp file)
      với mỗi bảng không thuộc danh sách loại trừ (D9): SELECT ... FORMAT SQLInsert, stream vào file (D5)
  + pub async fn restore(conn, database, path, watch: &dump::Watch)
      đọc incremental (D7), mỗi statement qua execute_check, dừng ở lỗi đầu tiên
  + fn strip_database_qualifiers(sql: &str, database: &str) -> String        (D4)
  + fn excluded_from_data_dump(engine: &str) -> bool                          (D9)

src-tauri/src/modules/db/commands/clickhouse.rs
  + clickhouse_dump(state, id, database, mode: String, path: String)
  + clickhouse_restore(state, id, database, path: String)
    (wiring giống postgres_dump/postgres_restore ở commands/postgres.rs: Transfer::start, reporter,
    dump::Watch — không in_background, xem D6)

src-tauri/src/modules/mod.rs
  + hai dòng generate_handler! cho clickhouse_dump, clickhouse_restore
```

## Frontend — file đổi

```
src/modules/db/sql/dialect.ts        + dumpRestoreWritable: boolean; sửa doc comment của writable (D8)
src/modules/db/mysql/dialect.ts      + dumpRestoreWritable: true
src/modules/db/postgres/dialect.ts   + dumpRestoreWritable: true
src/modules/db/sqlite/dialect.ts     + dumpRestoreWritable: true
src/modules/db/clickhouse/dialect.ts + dumpRestoreWritable: true   (writable vẫn false)
src/modules/db/clickhouse/api.ts     dump/restore: invoke() thật thay notSupported()
src/modules/db/sql/SqlWorkspace.tsx  + prop dumpRestoreReadOnly, DatabaseActions.disabled dùng nó thay readOnly (D8)
src/modules/db/DbTab.tsx             + tính dumpRestoreReadOnly, truyền cùng readOnly vào SqlWorkspace (D8)
src/modules/db/components/DatabaseActions/DatabaseActions.tsx
  sửa comment dòng 63-73 (không còn đúng "ClickHouse is null too... v1 has no dump or restore at all")
```

## Kiểm thử

**Rust, thuần** (`cargo test`, chạy CI):

- `strip_database_qualifiers`: `CREATE TABLE db.t (...)` → `CREATE TABLE t (...)`; `CREATE VIEW db.v
  AS SELECT * FROM db.t` → cả hai vị trí đều mất tiền tố; `CREATE MATERIALIZED VIEW db.mv TO db.tgt
  (...) AS SELECT ... FROM db.src` → cả ba vị trí; tên database trùng với một chuỗi literal trong
  statement (`WHERE name = 'db'`) → **không** bị đụng vào (chỉ vị trí `identifier.` mới bị bóc); tên
  database xuất hiện trong `ENGINE = Distributed('cluster', 'db', 'table')` → **giữ nguyên** (D4).
- `excluded_from_data_dump`: đúng danh sách D9, engine lạ không có trong danh sách → `false` (không
  loại trừ).
- Bộ quét statement incremental: cùng một file test-case đưa qua cả `split_statements` (toàn bộ một
  lần) lẫn splitter mới (chia làm nhiều chunk ở các ranh giới ngẫu nhiên, kể cả giữa chừng một quote)
  → phải ra cùng danh sách statement — bài test then chốt chứng minh hai splitter không lệch nhau.
  Thêm case chunk boundary rơi đúng giữa `''` (doubled single-quote escape) và giữa `\\` (backslash
  escape) — hai chỗ cần nhìn trước 1 ký tự mà state machine phải giữ được qua ranh giới chunk.
  `Tracker`'s progress: trọng số bằng nhau/khác nhau giữa các bảng cho ra `%` hợp lý.

**Bằng tay, ghi vào báo cáo cuối** (server thật `<old>_agent_test`, theo tiền lệ các spec ClickHouse
trước):

- Dump `all` một database có: bảng MergeTree thường, một `VIEW`, một `MATERIALIZED VIEW` có `TO`
  table riêng, một bảng data-skipping index — restore vào **một database khác tên** → mọi object lên
  đúng, view/MV query đúng dữ liệu (xác nhận D4's rewrite không chỉ đúng cú pháp mà còn đúng ngữ
  nghĩa sau khi database đổi tên).
- Nếu server test có sẵn hoặc dựng tạm một bảng `Distributed` trỏ ĐI đâu đó cụ thể: dump rồi restore
  vào database khác tên → xác nhận bảng Distributed vẫn trỏ đúng chỗ cũ (không bị rewrite theo D4's
  ngoại lệ), đồng thời KHÔNG có data nào được dump cho bảng này (D9).
- Bảng vài triệu dòng: dump `data`, theo dõi RAM của tiến trình không tăng vọt theo kích thước bảng
  (xác nhận D5 stream thật, không buffer). Restore lại file đó, theo dõi RAM tương tự (xác nhận D7).
- Bấm Cancel giữa lúc dump bảng lớn đang chạy → dump dừng, file dở dang bị xoá/báo lỗi rõ ràng (theo
  đúng hành vi 3 engine kia khi cancel) — vào `system.processes` trên server kiểm tra xem query có
  còn chạy tiếp sau khi client huỷ hay không (ghi kết quả thật vào báo cáo, dù kết quả là "vẫn chạy
  tiếp" — đó là D6's giới hạn đã biết, không phải điều kiện pass/fail).
- Đo kích thước một batch `FORMAT SQLInsert` mặc định trên bảng vài triệu dòng — xác nhận giả định
  D7 về "bộ nhớ đỉnh ~ một batch" bằng số thật, không chỉ suy luận từ tài liệu.

## Rủi ro

- **`DROP TABLE IF EXISTS` trước mỗi restore (D3) là hành vi phá hoại nếu chọn nhầm database.** Đã
  chấp nhận (khớp mysqldump), nhưng bảng ClickHouse thường là bảng phân tích rất lớn, tốn nhiều thời
  gian dựng lại hơn bảng OLTP điển hình — thiệt hại của một lần chọn nhầm lớn hơn ở MySQL. Không có
  safeguard bổ sung trong phase này (không nằm trong phạm vi câu hỏi đã hỏi); nếu về sau muốn thêm
  (liệt kê bảng sẽ bị DROP trước khi chạy, chẳng hạn), đó là một cải tiến UX riêng, không phải sửa
  thiết kế này.
- **Cancel không đảm bảo dừng query phía server (D6).** Đã ghi rõ trong Phi mục tiêu/D6 — rủi ro về
  kỳ vọng người dùng hơn là rủi ro kỹ thuật, miễn tài liệu/copy trong UI không hứa quá.
- **`excluded_from_data_dump` (D9) là danh sách tay, không đầy đủ tuyệt đối.** ClickHouse thêm engine
  mới liên tục; một engine tương lai không nằm trong danh sách nhưng cũng không phải kho dữ liệu thật
  sẽ bị thử dump data và có thể lỗi hoặc side-effect — chấp nhận, vì lựa chọn kia (whitelist chỉ cho
  engine đã biết) sẽ chặn nhầm những engine hợp lệ hiện có mà danh sách chưa liệt kê hết.
- **Bộ quét ranh giới statement incremental (D7) là phần rủi ro kỹ thuật lớn nhất của cả spec.** Nếu
  state machine giữ sai trạng thái qua ranh giới chunk (đặc biệt hai case nhìn-trước-1-ký-tự), một
  chunk boundary xui rủi rơi giữa quote có thể tách sai statement — hoặc tách một `INSERT` làm đôi
  (dữ liệu vào sai), hoặc gộp hai statement làm một (server báo lỗi cú pháp). Bài test bắt buộc so
  sánh kết quả giữa splitter cũ (toàn bộ) và splitter mới (chia chunk ngẫu nhiên) ở mục Kiểm thử chính
  là lưới an toàn cho rủi ro này — không merge nếu bài test đó chưa xanh với nhiều kiểu chia chunk.
- **Giả định `SHOW CREATE TABLE`/`DROP TABLE IF EXISTS` dùng chung được cho cả bảng thường lẫn
  VIEW/MATERIALIZED VIEW chưa verify bằng server thật.** Tài liệu ClickHouse gợi ý cả hai đều là
  dạng tổng quát (áp dụng được cho mọi object trong `system.tables`, `DROP VIEW`/`SHOW CREATE VIEW`
  chỉ là cú pháp tương đương), nhưng D3/D1 đang dựa trên giả định này chứ chưa chạy thật. Nếu sai,
  D3 cần rẽ nhánh theo loại object (`DROP VIEW IF EXISTS` cho View/MaterializedView) — sửa nhỏ, không
  đổi kiến trúc, nhưng phải verify trước khi viết plan chi tiết.
- **`FORMAT SQLInsert`'s batch size mặc định chưa verify bằng server thật** — nếu hoá ra nhỏ hơn nhiều
  so với tài liệu (hoặc một số phiên bản ClickHouse cũ hơn không hỗ trợ format này), giả định "bộ nhớ
  đỉnh ~ một batch" có thể sai lệch, hoặc phải fallback sang cách khác cho phiên bản cũ. Cần spike xác
  nhận sớm khi viết plan — nếu `FORMAT SQLInsert` không có trên phiên bản server tối thiểu app hỗ trợ,
  đây là điều phải quay lại thiết kế, không phải chi tiết vặt.

## Những gì để lại

- **`CREATE DICTIONARY`** — không thuộc `system.tables` theo cách app đọc; phase riêng nếu cần.
- **`KILL QUERY`/`KILL MUTATION` nối vào Cancel thật** — cần `query_id` tracking chưa có ở bất kỳ đâu
  trong app (đúng ghi chú đã có sẵn ở `dialect.cancellable` và D4 của spec row-writes).
- **Danh sách loại trừ D9 mở rộng/thu hẹp theo engine thực tế gặp phải** — không cố liệt kê hết mọi
  engine ClickHouse có thể có ngay từ đầu.
- **Safeguard trước khi DROP (liệt kê bảng sẽ mất, xác nhận thêm một bước)** — cải tiến UX, không
  phải phần của phase này.
