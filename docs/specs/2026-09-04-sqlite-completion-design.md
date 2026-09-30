---
status: implemented
date: 2026-09-04
---

# SQLite: dump dữ liệu + sửa cột đầy đủ (đóng nốt D3, D4)

Ngày: 2026-09-04

## Mục tiêu

Plan `2026-09-04-sqlite-db-kind.md` (gitignored) để lại đúng hai khoảng hở đã được
đặt tên và hoãn có chủ đích khi kind `sqlite` ra đời — D3 và D4. Cả hai đều còn nguyên trong code hôm
nay:

- **D3 — dump chỉ có structure.** [`sqlite_dump.rs:9-14`](../../apps/desktop/src-tauri/src/modules/db/drivers/sqlite_dump.rs#L9-L14):
  `SqlDumpMode::Data` và `All` bị từ chối thẳng bằng `error.sqliteDataDumpUnsupported`.
- **D4 — sửa cột chỉ đổi được tên.** [`sqlite_ddl.rs:242-247`](../../apps/desktop/src-tauri/src/modules/db/drivers/sqlite_ddl.rs#L242-L247):
  đổi kiểu/`NOT NULL`/default/collation đều bị từ chối bằng ba mã lỗi riêng.

Sau spec này: nút Dump cho SQLite mở đủ cả 3 mode (structure/data/all) như ba engine SQL kia; dialog
sửa cột trên SQLite cho đổi kiểu, `NOT NULL`, default, `COLLATE` — không chỉ đổi tên — bằng cách rebuild
bảng theo đúng quy trình 12 bước SQLite tự tài liệu hoá cho việc này. Đây là hai mảng độc lập nhau về
kỹ thuật, gộp một spec vì cùng một mục đích ("đóng nốt các khoảng hở đã biết của kind SQLite") và cùng
đụng một cặp file (`sqlite_dump.rs`, `sqlite_ddl.rs`).

## Phi mục tiêu

- **Không đổi hành vi `dump_structure` hiện có.** Nó không ghi `DROP TABLE IF EXISTS` trước mỗi
  `CREATE` (khác MySQL/Postgres/ClickHouse) — một bất đối xứng có thật nhưng không phải điều D3 từng
  hứa sửa; ghi lại ở mục Rủi ro, không đụng vào trong spec này.
- **Không thêm tiến trình/cancel thật cho `sqlite_script::run`.** Dùng nguyên, không sửa file đó —
  xem quyết định A5 và B8.
- **Không cho rebuild thêm/bớt PRIMARY KEY, hay đổi ràng buộc cấp bảng** (`CHECK`, `FOREIGN KEY`,
  `UNIQUE` khai theo kiểu table-constraint). `error.sqliteNoPrimaryKeyAfterwards` vẫn đứng nguyên cho
  index; rebuild có ranh giới tương tự — xem B6.
- **Không viết lại VIEW hay FOREIGN KEY ở bảng khác khi rebuild đổi tên cột.** Rebuild chỉ được gọi
  khi tên cột giữ nguyên (xem B1) — đổi tên vẫn đi qua nhánh `RENAME COLUMN` sẵn có, không qua rebuild.
- **Không giới hạn kích thước file dump**, giống ba engine kia.
- **`ATTACH` nhiều schema, tạo file mới, libSQL/Turso** — vẫn ngoài phạm vi, như plan gốc đã chốt.

## Hiện trạng

| Chỗ | Điều spec dựa vào |
| --- | --- |
| [`sqlite_dump.rs`](../../apps/desktop/src-tauri/src/modules/db/drivers/sqlite_dump.rs) | `dump_structure` đọc `sql` từ `sqlite_master` verbatim; `restore` đọc cả file vào RAM rồi gọi `sqlite_script::run` |
| [`sqlite_script.rs`](../../apps/desktop/src-tauri/src/modules/db/drivers/sqlite_script.rs) | `split_statements` (riêng, không `pub`) và `run` — bộ chạy script dùng chung với tab Query, không có hook tiến độ/cancel |
| [`sqlite.rs`](../../apps/desktop/src-tauri/src/modules/db/drivers/sqlite.rs) | `column_value` — đọc storage class thật của giá trị (`raw.type_info().name()`: `INTEGER`/`REAL`/`BLOB`/khác) thay vì kiểu khai báo của cột, đúng thứ D3 cần để sinh literal đúng; `quote_ident`, `split_default` |
| [`sqlite_ddl.rs`](../../apps/desktop/src-tauri/src/modules/db/drivers/sqlite_ddl.rs) | `column_definition` (dựng DDL một cột từ `ColumnSpec`), `quote_string`, `execute_all` (transaction), `modify_column` hiện tại (điểm sẽ rẽ nhánh sang rebuild) |
| [`sqlite_structure.rs`](../../apps/desktop/src-tauri/src/modules/db/drivers/sqlite_structure.rs) | `structure_columns` đọc `hidden` từ `pragma_table_xinfo` (`generated = hidden==2\|\|hidden==3`); `page_sizes` (riêng, không `pub`) đọc `dbstat` cho trọng số theo bảng — dùng lại được cho `Tracker` |
| [`dump.rs`](../../apps/desktop/src-tauri/src/modules/db/drivers/dump.rs) | `DumpMode` (structure/data/all), `Progress`, `Watch`, `Tracker` — hạ tầng chung ClickHouse đã dùng, SQLite hiện chưa đụng tới |
| [`clickhouse_dump.rs`](../../apps/desktop/src-tauri/src/modules/db/drivers/clickhouse_dump.rs) | Mẫu dump native không qua child process: `dump_data` stream từng bảng vào file qua `Tracker`, `restore` đọc incremental — SQLite không cần incremental (xem A5) nhưng noi theo cách wiring `Watch`/`Tracker` |
| [`commands/sqlite.rs`](../../apps/desktop/src-tauri/src/modules/db/commands/sqlite.rs) | `sqlite_dump`/`sqlite_restore` hiện không có `app: AppHandle`, không `Transfer::start`, không báo tiến độ — khác hẳn `commands/clickhouse.rs`'s `clickhouse_dump`/`clickhouse_restore` |
| [`commands/mod.rs`](../../apps/desktop/src-tauri/src/modules/db/commands/mod.rs) | `reporter(&app, &id)`, `Transfer::start`/`.flag()` — hạ tầng wiring dùng chung cho mọi kind |
| [`src/modules/db/sqlite/api.ts`](../../apps/desktop/src/modules/db/sqlite/api.ts) | `dump`/`restore` đã gọi đúng `invoke`, chỉ cần backend mở mode — **frontend không cần đổi gì cho phần A** |
| [`src/modules/db/components/TableStructure/TableStructure.tsx:718-723`](../../apps/desktop/src/modules/db/components/TableStructure/TableStructure.tsx) | Nút Edit đã bị `disabled` cho cột `generated`, dùng chung mọi engine — **frontend không cần đổi gì cho phần B**, dialog sửa cột đã cho nhập type/nullable/default/collation, chỉ backend đang chặn |
| [`src/modules/db/i18n/en.ts`, `vi.ts`](../../apps/desktop/src/modules/db/i18n) | `error.sqliteDataDumpUnsupported`, `error.sqliteColumnTypeUnchangeable`, `error.sqliteColumnNullUnchangeable`, `error.sqliteColumnDefaultUnchangeable` — ba mã sau xoá bỏ, mã đầu cũng xoá bỏ (đường đó không còn ai đi tới) |

## Phần A — Dump dữ liệu (D3)

### A1 — Sinh literal tự viết, không có tool ngoài, không có `FORMAT SQLInsert` để mượn.

Không giống ClickHouse (server tự sinh INSERT hộ qua `FORMAT SQLInsert`), SQLite không có gì tương
đương — plan gốc đã nói đúng: "đó là một bộ sinh SQL viết đúng, không phải một vòng lặp". Bốn lớp giá
trị, đọc theo **storage class thật của giá trị** (`raw.type_info().name()`, y hệt `column_value` đã
làm cho lưới dữ liệu — không đọc theo kiểu khai báo của cột, vì SQLite cho một cột `INTEGER` giữ một
chuỗi):

- `NULL` → từ khoá `NULL`, không quote.
- `INTEGER` → `i64` in thẳng bằng thập phân (`{}`).
- `REAL` → `f64` in bằng `{}` (Rust's Display cho `f64` đã round-trip được). Giá trị không hữu hạn
  (`NaN`/`Infinity`) — chỉ có thể xảy ra qua các hàm SQL đặc biệt, gần như không gặp trong dữ liệu
  thường — in ra `NULL` thay vì lỗi cả dump; ghi lại ở Rủi ro, không phải bỏ sót.
- `TEXT` → tái dùng `quote_string` của `sqlite_ddl.rs` (nới thành `pub(super)`): bọc `'...'`, nhân đôi
  dấu nháy đơn, không đụng backslash (SQLite không escape bằng backslash).
- `BLOB` → `x'` + hex thường (không cần crate mới, `format!("{:02x}", byte)` từng byte) + `'`. Chữ hoa
  hay thường đều hợp lệ với SQLite; chọn thường cho đơn giản.

### A2 — Cột nào vào INSERT: bỏ generated, bỏ hidden.

`SELECT`/`INSERT` chỉ lấy các cột có `hidden = 0` từ `pragma_table_xinfo` — loại cả `hidden = 1` (cột
ẩn của virtual table, ví dụ cột nội bộ FTS5) lẫn `hidden ∈ {2, 3}` (generated `VIRTUAL`/`STORED`, y hệt
điều kiện `generated` mà `structure_columns` đã dùng). Một cột generated không cần đọc giá trị: SQLite
tự tính lại khi restore chạy `CREATE TABLE` gốc (dump structure đã ghi nguyên xi biểu thức generated)
rồi `INSERT` các cột còn lại — không tự ý `INSERT` vào một cột generated, việc đó bị chính SQLite từ
chối.

### A3 — Một `INSERT` một dòng, không gộp nhiều dòng vào một `VALUES (...), (...)`.

Giống cách `sqlite3 .dump` tự làm. Đơn giản hơn để viết đúng (không phải tính giới hạn kích thước một
câu gộp), và khi restore lỗi giữa chừng, thông báo trỏ đúng một dòng dữ liệu thay vì một khối nhiều
dòng — khớp tinh thần `a_restore_that_fails_says_where_it_stopped` đã có sẵn cho structure.

### A4 — Stream từng dòng ra file, không buffer cả bảng vào RAM.

`sqlx::query(...).fetch(pool)` trả một stream; mỗi dòng đọc xong ghi thẳng ra file (qua
`tokio::io::BufWriter`/`AsyncWriteExt`) rồi bỏ. Không cần bước incremental-reader như ClickHouse's
restore (D7 ở spec ClickHouse) — SQLite ở đây là ghi ra, không phải đọc qua HTTP theo từng chunk byte.

### A5 — Restore: giữ nguyên `sqlite_script::run`, không sửa file `sqlite_script.rs`.

`restore()` đã đúng và đủ: một file dump `data`/`all` vẫn chỉ là SQL hợp lệ (CREATE + INSERT xen kẽ
hoặc tách khối), và bộ tách statement hiện có (`split_statements`, chưa `pub`, không đổi) xử lý được
mà không cần biết gì về "đây là một dump". **Không thêm hook tiến độ/cancel vào file này** — nó cũng là
bộ chạy "Run script" của tab Query; sửa nó để phục vụ riêng dump/restore là rủi ro hồi quy không cần
thiết cho một tính năng khác hẳn. Hệ quả: restore của SQLite vẫn không dừng giữa chừng khi bấm Cancel —
xem B8/Rủi ro, đối xứng với D6 của spec ClickHouse ("Cancel không đảm bảo dừng query phía server").

### A6 — `all` mode: structure trước, data nối tiếp — không đổi hành vi `dump_structure`.

Giữ nguyên `dump_structure` (không thêm `DROP TABLE IF EXISTS` — xem Phi mục tiêu). `dump_data` mở file
ở chế độ append khi `mode == All`, ghi từ đầu (truncate) khi `mode == Data`, giống hệt cách
`clickhouse_dump::dump_data`'s tham số `append` đã làm.

### A7 — Tiến độ: `Tracker` có trọng số theo `page_sizes` (đổi `pub(super)`), fallback đều nhau khi không có `dbstat`.

`sqlite_structure.rs::page_sizes` đã đọc `dbstat` cho đúng thứ `Tracker` cần làm trọng số — nới tầm
nhìn thành `pub(super)` và gọi lại từ `sqlite_dump.rs` thay vì viết lại truy vấn. Khi `dbstat` không có
(bản SQLite hệ thống không bật `SQLITE_ENABLE_DBSTAT_VTAB` — xem ghi chú đầu `sqlite_structure.rs`),
`page_sizes` đã tự trả rỗng và `Tracker::new` đã có sẵn nhánh "mọi phần nặng như nhau" khi tổng trọng
số bằng 0, không cần code thêm ở đây.

### Command wiring (`sqlite_dump`, `sqlite_restore`)

Cả hai thêm `app: AppHandle`, dùng `reporter(&app, &id)` + `Transfer::start(&state, &id)` +
`dump::Watch`, đúng mẫu `clickhouse_dump`/`clickhouse_restore` ở `commands/clickhouse.rs`. `sqlite_dump`
gọi `dump_structure` (khi mode ≠ Data) rồi `dump_data` (khi mode ≠ Structure) với `watch` xuyên suốt —
cả hai giờ nhận `watch: &dump::Watch` thay vì không nhận gì (dump_structure trước đây không có tham số
này; thêm vào, kể cả không dùng để cancel, để cùng `Tracker` báo tiến độ theo bảng cho nhất quán).
`sqlite_restore` đăng ký `Transfer` để nút Cancel không phải gọi vào một id không ai theo dõi, báo một
lần `Progress { percent: None, .. }` trước khi gọi `sqlite_script::run`, và trả về bình thường sau đó —
không có tiến độ giữa chừng, vì lý do đã nói ở A5.

## Phần B — Sửa cột đầy đủ qua rebuild bảng (D4)

### B1 — Rebuild chỉ kích hoạt khi tên cột không đổi; đổi tên vẫn đi nhánh cũ.

`modify_column` hiện tại: nếu `data_type`/`nullable`/`default_value` khác, từ chối ngay — dù tên có đổi
hay không. Sau spec này: nếu **tên không đổi** và bất kỳ trong bốn thứ (type, nullable, default,
collation) khác, rẽ sang rebuild. Nếu **tên có đổi** cùng lúc — dù có hay không có thay đổi khác — vẫn
từ chối như hôm nay, với một mã lỗi mới nói rõ lý do (`error.sqliteRenameWithOtherChanges`) thay vì một
trong ba mã cũ vốn không mô tả đúng tình huống này. Nếu tên không đổi và cả bốn thứ đều không đổi (dialog
gửi lại y hệt những gì đang có), giữ nguyên đường `Ok(())` sớm hiện có — không có gì để rebuild. Lý do tách riêng: một cột bị đổi tên có thể đang
được một VIEW hay ràng buộc ở bảng khác trỏ tới bằng tên cũ — B7 không viết lại những chỗ đó, nên gộp
"vừa đổi tên vừa rebuild" vào cùng một lần là mở rộng bề mặt rủi ro không cần thiết cho bản đầu. Người
dùng muốn cả hai làm hai lần: đổi tên trước (đi nhánh cũ), rebuild sau.

### B2 — Vá lại text `CREATE TABLE` gốc, không dựng lại từ metadata đã đọc.

Hai cách đã cân nhắc:

1. **Dựng lại `CREATE TABLE` từ `TableStructure`** (cột + index đã đọc được) — nguy cơ: mọi thứ
   `TableStructure` chưa mô hình hoá (`CHECK`, `FOREIGN KEY`, ràng buộc `UNIQUE`/`PRIMARY KEY` cấp
   bảng, `WITHOUT ROWID`, `STRICT`, biểu thức generated) sẽ bị bỏ rơi hoặc phải viết thêm chỗ đọc/ghi
   cho từng thứ — đúng kiểu "chi tiết sai" mà plan gốc cảnh báo là rủi ro mất dữ liệu lớn nhất.
2. **Vá text gốc** — đọc nguyên văn `CREATE TABLE` từ `sqlite_master` (đúng nguồn `dump_structure` đã
   tin cậy), chỉ thay đúng một mệnh đề cột, giữ nguyên mọi thứ khác — bao gồm cả những gì app không
   biết tới.

Chọn (2). Cùng một triết lý đã có ở `sqlite_dump.rs`: "lấy nguyên xi từ `sqlite_master`, không tự sinh
lại".

### B3 — Bộ tách mệnh đề: theo dấu phẩy ở độ sâu ngoặc 0, không phải một parser SQL đầy đủ.

Danh sách cột/ràng buộc bên trong cặp ngoặc ngoài cùng của `CREATE TABLE` được tách theo dấu phẩy ở độ
sâu ngoặc 0 — cùng tinh thần bộ quét ký tự `strip_database_qualifiers` (`clickhouse_dump.rs`) hay
`Scanner` (`clickhouse_script.rs`): nhận diện quote (`'`, `"`, `` ` ``, `[...]` — bốn kiểu quote SQLite
chấp nhận cho định danh), comment (`--`, `/* */`), và độ sâu ngoặc — để một `DECIMAL(10,2)` (dấu phẩy
trong ngoặc kiểu) không bị tách nhầm thành hai mệnh đề, còn dấu phẩy ngăn cách các cột thì bị tách đúng.
Không cần hiểu ngữ nghĩa từng token, chỉ cần biết "đây là ranh giới của một mệnh đề".

Mỗi mệnh đề tách ra được phân loại:

- Bắt đầu (không phân biệt hoa/thường) bằng `PRIMARY`, `UNIQUE`, `CHECK`, `FOREIGN` hoặc `CONSTRAINT`
  → ràng buộc cấp bảng, giữ nguyên văn, không đụng.
  - Nếu mệnh đề này chứa tên cột đang sửa (khớp định danh, có hoặc không quote) → **từ chối cả rebuild
    trước khi chạy gì**, mã `error.sqliteColumnInTableConstraint` — cột đó là một phần của khoá chính,
    ràng buộc unique, check hay khoá ngoại cấp bảng, và B2 đã chốt không viết lại ràng buộc.
- Ngược lại → mệnh đề cột. Tên cột là token đầu tiên (định danh trần hoặc có quote). Mệnh đề có tên
  khớp cột đang sửa (theo tên **gốc**, trước khi đổi) là mệnh đề bị thay thế.

Nếu không tìm thấy mệnh đề nào khớp tên cột (không nên xảy ra — `current_column` đã xác nhận cột tồn
tại trước đó — nhưng nếu `CREATE TABLE` dùng một cú pháp bộ tách này không nhận ra), từ chối với
`error.sqliteRebuildParseFailed` nói rõ tên bảng, thay vì chạy tiếp trên một danh sách cột sai.

### B4 — Mệnh đề thay thế: dựng bằng `column_definition` đã có, không viết thêm bộ sinh khác.

Mệnh đề cột cũ bị thay nguyên khối bằng `column_definition(&spec)` — đúng hàm `add_column` đang dùng,
nên quy tắc quote/escape/`DEFAULT`/`COLLATE` là một, không lệch giữa "thêm cột" và "sửa cột qua rebuild".

### B5 — So sánh collation thật trước khi quyết định rebuild, không giả định `None`.

`current_column` (đã có, dùng cho nhánh rename-only) chỉ đọc `type`/`nullable`/`default_value` — không
đọc collation, vì `pragma_table_xinfo` không có cột đó, và `StructureColumn.collation` trên toàn bộ
Structure tab của SQLite **luôn là `None`** (xem ghi chú đầu `sqlite_structure.rs`). Hậu quả nếu bỏ qua:
dialog sửa cột mở lên với ô collation rỗng cho một cột thật ra có `COLLATE NOCASE`; nếu người dùng chỉ
sửa type mà không đụng ô đó, `spec.collation` về `None`, và B4 thay nguyên mệnh đề cột bằng một bản
**không có `COLLATE`** — âm thầm xoá mất collation đang có, dù người dùng không hề định đổi nó. Đây là
đúng kiểu lỗi "chi tiết sai khi rebuild" mà plan gốc cảnh báo, chỉ khác là ở field collation thay vì ở
việc mất dữ liệu dòng.

Sửa: trước khi so sánh bốn field để quyết định có rebuild hay không, đọc **collation thật** của cột
đang sửa bằng cách chạy `split_column_clauses` (B3) trên `CREATE TABLE` gốc, lấy đúng mệnh đề của cột
đó, rồi tìm cụm `COLLATE <định danh>` cuối mệnh đề (nếu có) — quét trong phạm vi một mệnh đề đã tách
riêng, không phải toàn bộ câu `CREATE TABLE`, nên không cần lo về ranh giới ngoặc/quote nữa (B3 đã lo
việc đó khi tách). Không có `COLLATE` trong mệnh đề → collation hiện tại coi như `None`/`BINARY` (mặc
định của SQLite), khớp với việc `spec.collation` rỗng nghĩa là "không đổi gì" chứ không phải "xoá đi".

So sánh bốn field (type, nullable, default, collation-vừa-đọc-được) với bốn field spec gửi lên: khác
bất kỳ field nào → rebuild (B1); giống hệt cả bốn → `Ok(())` sớm, không rebuild.

### B6 — Chặn trước khi chạm gì: cột thuộc ràng buộc bảng (B3), cột generated, cột `INTEGER PRIMARY KEY`.

Ba trường hợp từ chối bằng tên, trước khi mở transaction:

- Cột nằm trong một ràng buộc cấp bảng (B3).
- Cột generated (`hidden ∈ {2,3}`) — UI đã chặn từ Structure tab, nhưng backend chặn lại cho chắc,
  đúng tinh thần "không tin frontend là hàng rào duy nhất" các file DDL khác trong module đã theo.
  Mã `error.sqliteColumnGenerated`.
- Cột là `INTEGER PRIMARY KEY` (bí danh rowid — `auto_increment` đã tính sẵn ở `structure_columns`,
  dùng lại điều kiện đó: `key_count == 1 && pk == 1 && type ILIKE 'integer'`). Đổi kiểu/nullable của
  chính rowid alias kéo theo đổi cả khoá chính — cùng tinh thần ranh giới đã vẽ cho index, nhưng
  **không** tái dùng `error.sqliteNoPrimaryKeyAfterwards`: chữ của nó nói về việc *thêm* một khoá chính
  vào bảng chưa có, sai ngữ cảnh khi cột đang sửa vốn *đã là* khoá chính. Mã riêng
  `error.sqliteColumnIsPrimaryKey` nói đúng điều đang xảy ra.

### B7 — Trình tự chạy: 12 bước SQLite tự tài liệu hoá, trên một connection tự `acquire`.

`PRAGMA foreign_keys` không đổi được khi đang trong transaction (SQLite bỏ qua lệnh đó nếu gọi giữa
chừng) và là thuộc tính của từng connection, không của file — nên toàn bộ chạy trên **một** connection
tự lấy ra khỏi pool (`pool.acquire()`), không phải `pool.begin()` trực tiếp như `execute_all` đang làm
cho các lệnh DDL khác trong file này:

1. Đọc `PRAGMA foreign_keys` hiện tại trên connection đó.
2. Nếu đang bật → tắt (`PRAGMA foreign_keys = OFF`) — ngoài transaction.
3. Đọc trước mọi `CREATE INDEX`/`CREATE TRIGGER` mà `sqlite_master` còn giữ cho bảng này
   (`tbl_name = table AND sql IS NOT NULL` — lọc thẳng ra index ngầm của ràng buộc inline, những cái đó
   tự sinh lại theo `CREATE TABLE` mới, không cần replay).
4. `BEGIN`.
5. `CREATE TABLE <tên tạm> (...)` — text đã vá ở B3/B4, cộng phần đuôi sau dấu ngoặc đóng cuối cùng của
   bản gốc (`WITHOUT ROWID`, `STRICT`, nếu có — giữ nguyên xi). Tên tạm: `"__<old>_rebuild_" + table`,
   kiểm tra trước không trùng bảng có sẵn (nếu trùng, thêm hậu tố số cho tới khi không trùng — cực
   hiếm nhưng rẻ để chắc chắn).
6. `INSERT INTO <tên tạm> (<danh sách cột KHÔNG generated, theo đúng thứ tự cột gốc>) SELECT
   <cùng danh sách đó> FROM <bảng gốc>` — danh sách cột tường minh hai lần (không `SELECT *`), để thứ
   tự và việc bỏ cột generated là chắc chắn chứ không phụ thuộc `new_X` xếp cột theo đúng thứ tự cũ.
7. `DROP TABLE <bảng gốc>`.
8. `ALTER TABLE <tên tạm> RENAME TO <bảng gốc>`.
9. Chạy lại từng câu `CREATE INDEX`/`CREATE TRIGGER` đã lưu ở bước 3, theo đúng thứ tự đọc được.
10. Nếu bước 1 đọc được `foreign_keys` đang bật → `PRAGMA foreign_key_check` trên bảng vừa dựng lại; có
    bất kỳ vi phạm nào → `ROLLBACK` toàn bộ transaction, trả lỗi `error.sqliteRebuildForeignKeyViolation`
    kèm bảng/cột vi phạm engine báo lại — không âm thầm bỏ qua.
11. `COMMIT`.
12. Nếu bước 2 có tắt → bật lại `PRAGMA foreign_keys = ON` — ngoài transaction, kể cả khi bước 10-11
    thất bại và đã rollback (đưa connection về đúng trạng thái ban đầu trong mọi nhánh, `finally`-style
    bằng cách bọc bước 4-11 và luôn chạy bước 12 sau đó bất kể `Result`).

Không dùng lại `execute_all` (nó `pool.begin()` thẳng, không có chỗ chèn bước 1-2 trước `BEGIN` trên
cùng connection) — viết một hàm riêng `rebuild_column` trong `sqlite_ddl.rs` cầm connection từ đầu tới
cuối.

### B8 — Cancel: không áp dụng, đúng như mọi DDL khác trong file này.

Rebuild là một lệnh DDL đồng bộ, giống `create_table`/`add_column`/... — không đi qua `Transfer`, không
có nút Cancel (dialect `cancellable: false` đã đóng khái niệm này cho toàn bộ SQLite từ trước, không
phải điều D4 mở ra hay cần bàn thêm).

## Backend — file đổi

```
src-tauri/src/modules/db/drivers/sqlite_structure.rs
  fn page_sizes  → pub(super)                                                          (A7)

src-tauri/src/modules/db/drivers/sqlite_ddl.rs
  fn quote_string  → pub(super)                                                        (A1)
  CurrentColumn / current_column  → + field collation: Option<String>, đọc qua
                       split_column_clauses thay vì pragma (B5)
  modify_column     → rẽ nhánh: tên đổi + gì khác cũng đổi → error mới (B1);
                       tên không đổi + gì khác đổi (kể cả collation, nay so sánh được thật — B5)
                       → gọi rebuild_column thay vì từ chối thẳng
  + fn rebuild_column(pool, table, name, spec) -> Result<(), AppError>                  (B2-B7)
  + fn split_column_clauses(create_table_sql: &str) -> Vec<Clause>                      (B3)
      bộ quét ký tự: quote ('/"/`/[]), comment (--, /* */), độ sâu ngoặc — dùng chung được
      cho cả việc tìm mệnh đề cần thay, phát hiện cột nằm trong ràng buộc bảng (B3), lẫn đọc
      collation thật của một cột (B5)
  + fn is_table_constraint(clause: &str) -> bool                                        (B3)
  + fn clause_collation(clause: &str) -> Option<String>                                 (B5)

src-tauri/src/modules/db/drivers/sqlite_dump.rs
  dump_structure(pool, path)                    → + watch: &dump::Watch (A6)
  + fn dump_data(pool, path, append: bool, watch: &dump::Watch) -> Result<(), AppError>  (A1-A4, A7)
  restore  → không đổi thân hàm (A5); chữ ký không đổi vì Transfer/report nằm ở command, không ở đây

src-tauri/src/modules/db/commands/sqlite.rs
  sqlite_dump    → + app: AppHandle; wiring Transfer::start/reporter/dump::Watch giống clickhouse_dump
  sqlite_restore → + app: AppHandle; Transfer::start + một lần report percent:None trước khi chạy (A5)
```

## Frontend — file đổi

Không có. `sqlite/api.ts` đã gọi đúng `invoke` với `mode` truyền thẳng xuống; `DumpDialog` đã cho chọn
cả 3 mode cho mọi kind trừ khi bị chặn riêng (SQLite không nằm trong danh sách chặn); dialog sửa cột đã
gửi đủ type/nullable/default/collation xuống `modifyColumn`. Toàn bộ thay đổi nằm ở việc backend không
còn từ chối những gì frontend đã gửi từ trước.

Xoá bốn khoá i18n không còn đường nào gọi tới: `error.sqliteDataDumpUnsupported`,
`error.sqliteColumnTypeUnchangeable`, `error.sqliteColumnNullUnchangeable`,
`error.sqliteColumnDefaultUnchangeable` (ở cả `en.ts` và `vi.ts`) — thêm sáu khoá mới:
`error.sqliteRenameWithOtherChanges`, `error.sqliteColumnInTableConstraint`, `error.sqliteColumnGenerated`,
`error.sqliteColumnIsPrimaryKey`, `error.sqliteRebuildParseFailed`, `error.sqliteRebuildForeignKeyViolation`.

## Kiểm thử

**Rust, thuần** (`cargo test`, chạy CI), tất cả trên `Fixture` có sẵn (`sqlite::tests::Fixture` —
fixture `author`/`post`/`tag`/`loose`/`recent` đã dùng khắp `sqlite_ddl.rs`/`sqlite_structure.rs`/
`sqlite_dump.rs`):

*Phần A:*
- Một dòng có đủ NULL, số nguyên, số thực, chuỗi có dấu nháy đơn, BLOB → dump rồi restore vào một
  database rỗng khác → dữ liệu đọc lại giống hệt byte-for-byte (BLOB) và giá trị-for-giá-trị (còn lại).
- Cột generated (`slug` của `post`, đã có sẵn trong fixture) không xuất hiện trong câu `INSERT` — dump
  rồi restore, giá trị generated ở bảng đích tự tính lại đúng, không phải giá trị chép từ nguồn.
- Bảng rỗng không sinh dòng `INSERT` nào (không lỗi, không dòng rác).
- `mode = all`: file có cả `CREATE TABLE` lẫn `INSERT`, tables trước data — restore vào database rỗng
  dựng lại đủ cả schema lẫn dữ liệu trong một lần.
- `mode = data` một mình: restore vào database đã có sẵn schema (không `CREATE`) → chỉ dữ liệu được
  nạp, không lỗi "table already exists".

*Phần B:*
- Đổi kiểu một cột có dữ liệu sẵn (`TEXT` → `INTEGER` trên một cột toàn chứa chuỗi số) → dữ liệu vẫn
  đọc lại đúng theo affinity mới, giống hệt kết quả nếu tự tay chạy 12 bước bằng `sqlite3` CLI.
- Thêm `NOT NULL` vào cột không có dòng nào NULL → thành công, cấu trúc phản ánh đúng.
- Thêm `NOT NULL` vào cột đang có ít nhất một dòng NULL → thất bại ở bước INSERT-SELECT, rollback toàn
  bộ, bảng gốc (`author`/`post`/...) còn nguyên — kiểm cả cấu trúc lẫn số dòng trước/sau bằng nhau.
- Đổi default → cấu trúc đọc lại đúng default mới; dòng cũ không bị viết đè giá trị (default chỉ áp
  dụng cho INSERT sau này, không phải UPDATE hàng loạt).
- Thêm/đổi `COLLATE` → cấu trúc phản ánh đúng, một `ORDER BY` trên cột đó sau rebuild sắp xếp theo
  collation mới.
- Index và trigger của bảng (fixture `post` có `post_author`, `post_title`) còn nguyên sau rebuild —
  đọc lại `table_structure` thấy đủ, và (với trigger, nếu fixture có) hành vi trigger vẫn chạy đúng sau
  rebuild.
- Cột nằm trong ràng buộc cấp bảng (viết một fixture phụ có `UNIQUE(a, b)` kiểu table-constraint) →
  `rebuild_column` từ chối với `error.sqliteColumnInTableConstraint`, không chạm gì tới bảng.
- Cột `INTEGER PRIMARY KEY` (`id` của mọi bảng fixture) → từ chối `error.sqliteColumnIsPrimaryKey`.
- Cột generated (`slug`) → từ chối `error.sqliteColumnGenerated`, dù về lý thuyết gọi thẳng hàm này bỏ
  qua UI.
- Vừa đổi tên vừa đổi kiểu trong một lần gọi → từ chối `error.sqliteRenameWithOtherChanges`, không chạy
  gì (B1).
- `split_column_clauses`: một `CREATE TABLE` có cột kiểu `DECIMAL(10,2)` (dấu phẩy trong ngoặc kiểu),
  một default là biểu thức có dấu phẩy (ví dụ `DEFAULT (max(0, 1))`), một comment `--` chứa dấu phẩy,
  một tên cột quote bằng `` ` ``/`"`/`[]` chứa khoảng trắng — tách đúng số mệnh đề, không tách nhầm bên
  trong ngoặc/quote/comment.
- `WITHOUT ROWID` (một fixture phụ) → rebuild giữ nguyên hậu tố đó ở `CREATE TABLE` mới, bảng sau
  rebuild vẫn là `WITHOUT ROWID` (đọc qua `PRAGMA table_info`'s hành vi hoặc so `sqlite_master`.sql
  hậu-rebuild có chứa cụm đó).

**Bằng tay, ghi vào báo cáo cuối** (theo tiền lệ các spec ClickHouse — nhưng SQLite không có server
test riêng trong danh sách bộ nhớ, dùng một file `.db` tự tạo trong scratchpad):

- Mở app thật, dump `all` một file `.db` có vài nghìn dòng, restore vào một file mới, so hai file bằng
  mắt qua UI (số dòng, vài giá trị mẫu) — xác nhận đường vòng qua UI (dialog chọn mode, progress bar,
  không bị kẹt ở "đang xử lý") hoạt động, không chỉ hàm Rust đơn lẻ.
- Sửa một cột qua dialog Structure tab thật (đổi kiểu, thêm NOT NULL) trên một bảng đang có dữ liệu,
  xác nhận UI không cho bấm Edit trên cột generated (đã đúng từ trước, xác nhận lại không bị B1-B7 làm
  hỏng) và dialog sau khi lưu đọc lại đúng cấu trúc mới.

## Rủi ro

- **`sqlite_master.sql` không phải lúc nào cũng là cú pháp `split_column_clauses` lường hết được.**
  SQLite cho khá nhiều biến thể cú pháp cột hợp lệ (kiểu có hay không có, generated với hai cách viết
  `GENERATED ALWAYS AS (...) STORED/VIRTUAL` hoặc chỉ `AS (...) STORED/VIRTUAL`, default là biểu thức
  lồng ngoặc sâu nhiều lớp). B3's `error.sqliteRebuildParseFailed` là lưới an toàn khi bộ tách không
  chắc — chấp nhận từ chối rebuild trong một số trường hợp cạnh hiếm hơn là chạy một mệnh đề bị cắt sai.
- **`INSERT INTO new_X SELECT ... FROM X` là bước có thể chậm trên bảng lớn**, khoá ghi cả file trong
  lúc chạy (transaction duy nhất, không transaction phụ nào chen được). SQLite ở đây là file cục bộ nên
  chấp nhận được — cùng lý lẽ `table_stats`'s `COUNT(*)` mỗi bảng đã chấp nhận trước đó — nhưng một
  file vài GB sẽ thấy rebuild "đứng hình" một lúc, không có progress bar (B8 đã chốt không cần).
- **Giá trị `REAL` không hữu hạn bị dump thành `NULL` (A1)** — mất thông tin thật nếu ai đó cố tình lưu
  `NaN`/`Infinity` (hiếm, chỉ qua hàm SQL đặc biệt, không qua đường nhập liệu thường của app). Ghi nhận
  là giới hạn đã biết, không phải bỏ sót.
- **Cancel không dừng được restore giữa chừng (A5), và rebuild của D4 hoàn toàn không có Cancel (B8).**
  Đối xứng với D6 của ClickHouse — chấp nhận, miễn UI không hứa quá (nút Cancel vẫn hiện cho restore vì
  `dumpRestoreWritable` chung cho dump lẫn restore, dù bấm vào gần như không có tác dụng cho tới khi
  `sqlite_script::run` trả về).
- **`dump_structure` vẫn không có `DROP TABLE IF EXISTS`** (bất đối xứng với 3 engine kia, xem Phi mục
  tiêu) — restore một dump `all`/`structure` vào một database đã có bảng cùng tên vẫn lỗi "table already
  exists" thay vì ghi đè sạch. Không sửa trong spec này; nếu muốn đồng bộ hành vi, đó là một thay đổi
  hành vi restore cần hỏi lại người dùng trước (ảnh hưởng tới dữ liệu đã restore trước đó), không phải
  một sửa nhỏ đi kèm D3/D4.

## Những gì để lại

- **`DROP TABLE IF EXISTS` cho SQLite dump** — xem Rủi ro, cần quyết định riêng.
- **Cancel thật cho restore/rebuild** — cần hook tiến độ/huỷ vào `sqlite_script::run`, việc đó ảnh
  hưởng cả tab Query; để dành nếu sau này có nhu cầu rõ ràng.
- **Rebuild cho phép đổi tên cùng lúc với đổi kiểu** (B1) — có thể mở sau nếu người dùng thấy phải bấm
  Edit hai lần là phiền, lúc đó mới đáng để giải quyết bài toán "viết lại VIEW/FOREIGN KEY tham chiếu
  tên cũ".
- **Thêm/bớt PRIMARY KEY hay ràng buộc cấp bảng qua rebuild** — biên đã vẽ ở B1/B6/Phi mục tiêu, vẫn
  đứng nguyên; đây là bước rebuild phức tạp hơn hẳn (đụng tới toàn bộ index/khoá ngoại trỏ vào bảng),
  không phải một mở rộng nhỏ của D4.
