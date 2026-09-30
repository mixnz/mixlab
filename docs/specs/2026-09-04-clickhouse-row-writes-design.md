---
status: implemented
date: 2026-09-04
---

# ClickHouse: ghi dòng qua lưới (insert / update / delete)

Ngày: 2026-09-04

## Mục tiêu

ClickHouse (`6f72375`/`0127031`, v1) chỉ đọc. Spec này mở đúng một mảng trong ba mảng CHANGELOG
còn ghi nợ ("no editing, no DDL, no dump/restore"): **editing** — lưới dữ liệu trên Data tab được
phép insert/update/delete dòng, giống MySQL/PostgreSQL/SQLite. DDL (Structure tab) và dump/restore
vẫn đóng, không đổi.

Sau khi làm xong: mở một bảng ClickHouse, sửa một ô rồi rời ô đó → dòng được cập nhật thật trên
server. Chọn dòng, bấm Delete → dòng biến mất. Bấm "Add row", điền, lưu → dòng mới xuất hiện. "Add
table", Structure tab, Query tab, dump/restore vẫn đóng y như trước — CHANGELOG dòng ClickHouse sẽ
đổi thành "editing" đã có, DDL/dump vẫn chưa.

## Phi mục tiêu

- **Query tab vẫn đóng hoàn toàn**, kể cả gõ tay `INSERT`/`UPDATE`/`DELETE`. Không đụng
  `guard.ts`. Lý do ở D6.
- **Structure tab, "Add table", `DatabaseActions` (dump/restore/drop) không đổi** — vẫn khoá bởi
  `dialect.writable = false`. `clickhouse/editing.ts` vẫn là shape rỗng.
- **Không sửa engine không phải MergeTree family riêng** (View, Distributed, engine Log, Kafka...).
  `ALTER TABLE UPDATE/DELETE`/`TRUNCATE` không chạy được trên một số engine — để ClickHouse tự báo
  lỗi, không đoán trước bằng cách đọc `system.tables.engine`.
- **Không cursor-based paging, không sửa D13 của plan v1.**
- **Không thêm khái niệm "row đang pending mutation" trong UI** (spinner riêng, badge "đang áp
  dụng nền"...). Promise của `updateRow`/`deleteRows` chỉ resolve khi mutation xong (D2) — lưới
  không cần biết gì thêm.

## Hiện trạng

| Chỗ | Điều spec dựa vào |
| --- | --- |
| [`src-tauri/src/modules/db/drivers/clickhouse.rs`](../../apps/desktop/src-tauri/src/modules/db/drivers/clickhouse.rs) | `Connection` (rẻ, không phải pool), `query_with_params` (bind `{name:Type}` qua URL), `execute_check` (statement không có `FORMAT JSON`), `is_decodable`, `table_columns`, `quote_ident`, `qualified` |
| [`src-tauri/src/modules/db/commands/clickhouse.rs`](../../apps/desktop/src-tauri/src/modules/db/commands/clickhouse.rs) | Chỉ các lệnh đọc; `clickhouse_connection` helper ở `commands/mod.rs:416` |
| [`src-tauri/src/modules/db/drivers/mysql.rs:579,697`](../../apps/desktop/src-tauri/src/modules/db/drivers/mysql.rs) | `update_row` (pre-check `matched=1`, `<=>` cho NULL, transaction thật), `delete_rows` (`DELETE...LIMIT 1` per key, `all` → `DELETE` không WHERE) — hợp đồng phải khớp, cơ chế không copy được nguyên vì ClickHouse không có transaction, không có `LIMIT` trên mutation |
| [`src/modules/db/sql/api.ts`](../../apps/desktop/src/modules/db/sql/api.ts) | `updateRow(id,db,table,updates,key)`, `insertRows(id,db,table,rows)`, `deleteRows(id,db,table,keys,all,resetAutoIncrement)` — chữ ký không đổi |
| [`src/modules/db/clickhouse/api.ts`](../../apps/desktop/src/modules/db/clickhouse/api.ts) | Ba method hiện `notSupported()` — đổi thành `invoke()` thật |
| [`src/modules/db/components/SqlTable/SqlTable.tsx:834,950-957`](../../apps/desktop/src/modules/db/components/SqlTable/SqlTable.tsx) | `rowKey`: `primaryKey.length > 0 ? primaryKey : columns` — ClickHouse luôn báo `primaryKey: []` nên đã tự dùng toàn bộ cột làm key, không cần đổi gì ở đây |
| [`src/modules/db/sql/dialect.ts:131`](../../apps/desktop/src/modules/db/sql/dialect.ts) | `writable: boolean` — một cờ khoá chung DDL + rows + Query tab; cần tách |
| [`src/modules/db/DbTab.tsx:767`](../../apps/desktop/src/modules/db/DbTab.tsx) | `readOnly={... || !engine.dialect.writable}` — chỗ duy nhất tính cờ này, truyền thẳng vào `SqlWorkspace` |
| [`src/modules/db/sql/SqlWorkspace.tsx:480,703`](../../apps/desktop/src/modules/db/sql/SqlWorkspace.tsx) | Một `readOnly` prop rẽ tới `SqlTable`, `TableStructure`, `QueryEditor`, sidebar "Add table", `DatabaseActions` — cần rẽ hai nhánh |
| plan v1 `2026-09-04-clickhouse-db-kind.md` (local, never committed) | D7 (whitelist decodable), D13 (paging), Phi mục tiêu gốc — nguồn của mọi giới hạn spec này thừa hưởng |

**Ba điều đo được từ việc đọc `mysql.rs`, quyết định thiết kế:**

1. **MySQL's `update_row` có pre-check `SELECT COUNT(*) WHERE <key>`, từ chối nếu `matched != 1`.**
   ClickHouse không có PK — bảng có thể có nhiều dòng giống hệt nhau trên mọi cột. Không có bước
   này, sửa/xoá "một dòng" có thể chạm nhiều dòng.
2. **MySQL's `delete_rows` dùng `DELETE ... LIMIT 1` làm lưới an toàn thứ hai.** `ALTER TABLE ...
   DELETE WHERE` của ClickHouse không có `LIMIT`. Với dòng trùng lặp, pre-check ở mục 1 là lưới an
   toàn *duy nhất* có thể có — không phải lớp phòng thủ phụ như bên MySQL.
3. **`build_where` (đọc) đã dùng `{name:Type}` với kiểu gốc cho `gt/gte/lt/lte`, chỉ `eq` mới bọc
   `toString()`.** Xác nhận: so sánh trực tiếp theo kiểu gốc là mẫu có sẵn, được test
   (`orders_by_the_columns_own_type_rather_than_text`), dùng được cho WHERE key của ghi — không
   cần bọc `toString()` cho mọi cột.

## Quyết định đã chốt

**D1 — Định danh dòng: không đổi gì ở frontend.**
`rowKey`/`primaryKey.length > 0 ? primaryKey : columns` đã tự rơi vào nhánh "toàn bộ cột" vì
`primary_key` của ClickHouse luôn rỗng. `updateRow`/`deleteRows` nhận `key`/`keys` là map tên cột →
giá trị hiển thị (`normalizeCellValue`, tức `String(raw)` không làm tròn) — y hệt shape MySQL/
Postgres đã nhận.

**D2 — WHERE key: so sánh trực tiếp theo kiểu gốc cho cột decodable, `toString()` chỉ cho cột
không decodable.**
Hàm mới `build_key_where(columns: &BTreeMap<String,String>, key: &Map<String,Value>)`, dùng lại
`is_decodable` đã có (D7 của plan v1):

```rust
for (name, value) in key {
    let ty = columns.get(name)?;
    match value {
        Value::Null => clauses.push(format!("{} IS NULL", quote_ident(name))),
        v if is_decodable(ty) => clauses.push(format!(
            "{} = {}", quote_ident(name), placeholder(ty, v.as_display_text())
        )),
        v => clauses.push(format!(
            "toString({}) = {}", quote_ident(name), placeholder("String", v.as_display_text())
        )),
    }
}
```

Lý do đổi so với đề xuất ban đầu (bọc `toString()` mọi cột, mượn nguyên `build_where`'s `eq`):
`toString()` trên cột thuộc sorting key chặn ClickHouse dùng sparse primary index — mutation quét
cả phần dữ liệu thay vì một dải hẹp; và với `Float32/64`, `toString()` round-trip qua JSON rồi qua
`String(raw)` của JS không đảm bảo khớp lại byte-for-byte chuỗi hiển thị. Rủi ro còn lại (không
match được vì lệch định dạng hiếm gặp) được D3 biến thành lỗi rõ ràng thay vì ghi sai âm thầm.
`NULL` dùng `IS NULL` vì ClickHouse không có `<=>`.

**D3 — Pre-check `matched = 1` bắt buộc cho cả update lẫn delete-by-key (không phải tuỳ chọn).**
Trước khi `ALTER TABLE ... UPDATE/DELETE`, chạy `SELECT count() FROM t WHERE <key-clause>`. Khác 1
→ `Err(err!("error.rowsMatched", matched = n))` (key rỗng dùng chung `error.updateWithoutKey`/
`error.deleteWithoutKey` — cả ba key tiếng Anh này đã có sẵn, dùng chung với MySQL/Postgres/
SQLite, không thêm key mới). Đây là lưới an toàn duy nhất khả thi trên ClickHouse (mục 1, 2 ở
trên) — không transaction để rollback nếu sai, không `LIMIT` để tự giới hạn thiệt hại.

Xoá nhiều dòng cùng lúc (`deleteRows` với nhiều `keys`, `all: false`): gộp thành **một** mutation
`ALTER TABLE t DELETE WHERE (k1) OR (k2) OR ...` thay vì lặp như MySQL, pre-check
`SELECT count() WHERE (k1) OR (k2) OR ...` phải bằng đúng `keys.len()` — khác đi thì từ chối toàn
bộ batch (một dòng đã biến mất, hoặc trùng lặp làm phồng số đếm, đều là lý do dừng chứ không đoán
tiếp). Đổi từ "lặp per-key" sang "gộp một mutation" vì ClickHouse không có gì tương đương `LIMIT 1`
để làm an toàn cho lặp — an toàn phải đến từ pre-check, và pre-check gộp rẻ hơn N lần poll mutation
riêng lẻ khi người dùng chọn nhiều dòng.

**D4 — `UPDATE`/`DELETE` là mutation bất đồng bộ: gửi xong, chờ tới khi `is_done`, có timeout.**
`ALTER TABLE` chỉ enqueue. Để giữ đúng hợp đồng hiện tại của `SqlApi` (Promise resolve = đã ghi
xong, lưới refetch thấy ngay), backend tự poll trước khi trả lời frontend:

1. Trước khi gửi `ALTER`, đọc `mutation_id` hiện có của `(database, table)` từ `system.mutations`
   làm baseline.
2. Gửi `ALTER` qua `execute_check` (không `FORMAT JSON` — mutation không trả rows).
3. Poll `system.mutations WHERE database=? AND table=?`, tìm dòng có `mutation_id` không nằm trong
   baseline. Tìm thấy đúng một dòng mới → theo dõi `is_done`/`latest_fail_reason` của riêng nó, mỗi
   200ms, tối đa 30s.
   - `is_done=1`, `latest_fail_reason` rỗng → `Ok(())`.
   - `latest_fail_reason` khác rỗng → `Err`, kèm chuỗi đó (mutation thất bại — ví dụ type mismatch
     khi SET một cột không decodable).
   - Hết 30s chưa xong → `Err(err!("error.clickhouseMutationTimeout"))`, khoá mới, tiếng Anh nói rõ
     "vẫn có thể đang chạy nền, thử tải lại bảng sau". Không coi là thành công giả.
   - Không tìm thấy đúng một dòng mới (0 hoặc >1 — mutation khác chen vào cùng lúc từ nơi khác) →
     tiếp tục poll cho tới timeout, không đoán.

**Sửa lại so với bản đầu (đã xác minh sai bằng server thật, xem Kiểm thử):** đề xuất ban đầu định
so khớp thêm bằng `command` — cột `system.mutations.command` đúng bằng text vừa gửi. Kiểm tra trên
server thật (`clickhouse-test-server`, ClickHouse 26.8) cho thấy điều đó sai: ClickHouse tự format
lại câu lệnh trước khi lưu — bỏ backtick quanh tên cột, bọc thêm ngoặc quanh mỗi vế `AND`/`OR`. Gửi
`` UPDATE name = 'x' WHERE `id` = '2' `` được lưu thành `(UPDATE name = 'x' WHERE (id = '2'))`. So
khớp text theo cách cũ sẽ không bao giờ khớp, khiến mọi update/delete timeout 30 giây rồi báo lỗi dù
đã ghi thành công trên server. Sửa: chỉ so khớp bằng `mutation_id` không nằm trong baseline — không
cần so command nữa, vì tập hợp `mutation_id` mới đã đủ xác định "mutation này là của lời gọi này",
với cùng giới hạn đã biết trước (mutation khác chen ngang trong đúng khoảng thời gian đó).

`INSERT` **không** đi qua cơ chế này — `INSERT` trên ClickHouse là đồng bộ, không phải mutation.

**D5 — Xoá toàn bộ bảng (`all: true`) dùng `TRUNCATE TABLE`, đồng bộ, không qua D4.**
Né hẳn việc poll cho trường hợp phổ biến nhất. `resetAutoIncrement` no-op — giữ tham số trong chữ
ký lệnh (khớp tên `invoke` gửi, `#[allow(unused_variables)]` như `run_id` của
`clickhouse_run_script` đã làm) nhưng không dùng: ClickHouse không có khái niệm này.

**D6 — Frontend: tách readOnly của riêng lưới dữ liệu, không đụng `guard.ts`, không mở Query tab.**
Thay vì thêm state máy hai chiều (`writable`/`rowsWritable` độc lập, có thể lệch nhau) và tổng quát
hoá `writingStatements()` sang tập verb — bỏ, quá tay cho phase này. Chỉ cần:

- `SqlDialect` thêm `rowsWritable: boolean`. MySQL/Postgres/SQLite: `true` (đã ghi được, không đổi
  hành vi). ClickHouse: `true` — thay đổi duy nhất của phase này ở tầng dialect.
- `writable` giữ nguyên nghĩa, đổi thành *chỉ* còn gác DDL/dump/restore/Query tab — doc comment sửa
  lại. ClickHouse: vẫn `false`.
- `DbTab.tsx`: thêm `dataReadOnly={(activeSavedConnection?.readOnly ?? false) ||
  !engine.dialect.rowsWritable}` bên cạnh `readOnly` hiện có, cả hai truyền vào `SqlWorkspace`.
- `SqlWorkspace.tsx`: prop mới `dataReadOnly`, chỉ chuyển vào `SqlTable`'s `readOnly`
  ([SqlWorkspace.tsx:703](../../apps/desktop/src/modules/db/sql/SqlWorkspace.tsx#L703)). `TableStructure`,
  `QueryEditor`, sidebar "Add table", `DatabaseActions` giữ nguyên `readOnly` cũ — không đổi.

Hệ quả đúng như Phi mục tiêu: gõ `UPDATE`/`DELETE` tay trong Query tab trên ClickHouse vẫn bị
`guard.ts::writingStatements` chặn, vì `QueryEditor` vẫn nhận `readOnly` (từ `writable=false`), chưa
đổi.

**D7 — `insert_rows`: một câu `INSERT` duy nhất khi mọi dòng cùng tập cột; từ chối thẳng khi khác.**
`SqlApi.insertRows`'s doc hứa "một transaction — một dòng bị từ chối thì không dòng nào vào".
ClickHouse không có multi-statement transaction nên không giữ được lời hứa đó cho N câu `INSERT`
riêng như MySQL làm. Một câu `INSERT INTO t (cols) VALUES (...), (...), ...` **là** atomic (một
block ghi), nhưng chỉ khi mọi dòng khai đúng một tập cột giống nhau. Rows đến với tập cột khác nhau
→ `Err(err!("error.clickhouseHeterogeneousInsert"))` (khoá mới) thay vì âm thầm ghi từng phần —
tường minh còn hơn vi phạm hợp đồng "all or nothing" mà không nói ra. `INSERT` không qua D3/D4 (là
DML đồng bộ bình thường, không phải mutation, không cần pre-check vì không có gì để khớp trước).

**D8 — Giá trị ghi (SET/VALUES) gửi nguyên text đã nhập, không ép kiểu ở tầng này.**
Cùng tinh thần D7 của plan v1 ("không cố sửa ở tầng đọc"): áp dụng cho ghi. Cột không decodable
(Array/Map/...) mà người dùng sửa → text gửi thẳng, ClickHouse tự báo lỗi type mismatch nếu không
hợp. Không thêm cơ chế "khoá ô không cho sửa" ở frontend cho các cột này — không có chỗ nào trong
`SqlTable.tsx` hiện gác việc sửa ô theo kiểu cột (kể cả `isGenerated` cũng chỉ ảnh hưởng copy-as-
INSERT, không ảnh hưởng sửa ô trực tiếp), nên không phát minh cơ chế mới riêng cho ClickHouse.

## Backend — file đổi

```
src-tauri/src/modules/db/drivers/clickhouse.rs
  + build_key_where(columns, key) -> Result<(String, Vec<(String,String)>), AppError>   (D2)
  + matched_count(conn, table_ref, where_clause, params) -> Result<i64, AppError>        (D3)
  + run_mutation_and_wait(conn, database, table, alter_sql) -> Result<(), AppError>       (D4)
  + pub async fn update_row(conn, database, table, updates: &Map<String,Value>, key: &Map<String,Value>)
  + pub async fn insert_rows(conn, database, table, rows: &[Map<String,Value>])           (D7)
  + pub async fn delete_rows(conn, database, table, keys: &[Map<String,Value>], all: bool, reset_auto_increment: bool)

src-tauri/src/modules/db/commands/clickhouse.rs
  + clickhouse_update_row, clickhouse_insert_rows, clickhouse_delete_rows
    (chữ ký giống hệt mysql_update_row/mysql_insert_rows/mysql_delete_rows ở commands/mysql.rs)

src-tauri/src/modules/mod.rs
  + ba dòng generate_handler! cho ba lệnh trên
```

## Frontend — file đổi

```
src/modules/db/sql/dialect.ts        + rowsWritable: boolean; sửa doc comment của writable (D6)
src/modules/db/mysql/dialect.ts      + rowsWritable: true
src/modules/db/postgres/dialect.ts   + rowsWritable: true
src/modules/db/sqlite/dialect.ts     + rowsWritable: true
src/modules/db/clickhouse/dialect.ts + rowsWritable: true   (writable vẫn false)
src/modules/db/clickhouse/api.ts     updateRow/insertRows/deleteRows: invoke() thật thay notSupported()
src/modules/db/DbTab.tsx             + dataReadOnly, truyền cùng readOnly vào SqlWorkspace (D6)
src/modules/db/sql/SqlWorkspace.tsx  + prop dataReadOnly, chỉ SqlTable dùng nó thay readOnly (D6)
```

## Kiểm thử

**Rust, thuần** (`cargo test`, chạy CI):

- `build_key_where`: cột decodable → `{}= {pN:<kiểu gốc>}`; cột không decodable →
  `toString({}) = {pN:String}`; giá trị `null` → `{} IS NULL`, không sinh placeholder; trộn cả ba
  trong một key → nối bằng `AND` đúng thứ tự cột được đưa vào map.
- `insert_rows`'s phần dựng câu (tách khỏi phần gửi HTTP, test thuần): mọi dòng cùng cột → một câu
  multi-VALUES; khác cột → lỗi trước khi gửi gì cả.
- Chuỗi lỗi `matched != 1`, `mutation timeout` giữ đúng khoá i18n đã đặt trong D3/D4.

**Bằng tay, ghi vào báo cáo cuối** (theo đúng tiền lệ T3 của plan v1 — server thật, không CI, dùng
`clickhouse-test-server` trong memory, ví dụ throwaway `cargo run --example`, xoá sau khi xong):

- Sửa một ô trên bảng có sorting key → dòng cập nhật đúng, `EXPLAIN` xác nhận mutation không quét
  toàn bảng khi WHERE rơi trúng cột sorting key (xác nhận giả định của D2).
  bảng có Nullable, sửa dòng có NULL ở cột khác cột đang sửa → vẫn khớp (xác nhận D2's `IS NULL`).
  bảng có Array/Map, sửa cột khác trên cùng dòng → vẫn khớp được nhờ `toString()` trên cột đó
  (xác nhận D2).
- Tạo 3 dòng giống hệt nhau (mọi cột) → sửa một trong ba qua lưới → phải bị chặn bằng
  `error.rowsMatched` (matched=3), không sửa nhầm. Xoá một trong ba → cùng vậy.
- Đo thời gian một mutation thật xong (`is_done=1`) trên bảng cỡ vài triệu dòng — xác nhận 30s đủ
  hay phải chỉnh (D4's ghi chú "cần xác minh").
- `insertRows` nhiều dòng cùng cột → một `INSERT`, atomic thật (thử ép một dòng lỗi kiểu, xác nhận
  không dòng nào trong batch đó lọt vào bảng).
- Query tab: gõ tay `UPDATE`/`INSERT` trên kết nối ClickHouse → vẫn bị `guard.ts` chặn như trước
  (xác nhận D6 không vô tình mở Query tab).

## Rủi ro

- **Nhận nhầm mutation_id (D4).** Hai mutation nộp gần như đồng thời trên cùng bảng (từ the standalone client hoặc
  từ nơi khác) có thể làm bước "tìm mutation_id mới" ra 0 hoặc >1 kết quả — trường hợp đó tiếp tục
  poll tới timeout thay vì đoán (đã sửa ở D4). **Đã xác minh bằng server thật** (`clickhouse-test-
  server`, ClickHouse 26.8): round-trip insert → update (kể cả khớp qua `IS NULL`) → delete, cộng
  pre-check `matched=1` chặn đúng khi có ba dòng giống hệt nhau, và `all: true` dùng `TRUNCATE` —
  tất cả chạy đúng qua code Rust thật, không chỉ unit test thuần. Việc xác minh này cũng là lý do
  D4 đổi từ so khớp `command` text (sai — xem D4) sang chỉ so khớp `mutation_id`. Rủi ro còn lại
  (mutation khác chen ngang đúng lúc) là thật nhưng hiếm, chưa có cách kiểm chứng rẻ hơn.
- **Xoá/sửa nhiều dòng cùng lúc dồn vào một `ALTER TABLE ... WHERE (...) OR (...) OR ...` dài** khi
  người dùng chọn rất nhiều dòng (vài trăm) — WHERE clause dài, mỗi nhánh có thể là toàn bộ cột của
  bảng. Chưa đo ảnh hưởng thật; nếu chậm rõ rệt, có thể cần giới hạn số dòng chọn được xoá cùng lúc
  qua lưới (không nằm trong phạm vi phase này, ghi lại nếu gặp).
- **30 giây timeout (D4) có thể ngắn với bảng rất lớn hoặc mutation nặng** (sửa cột nằm trong
  sorting key buộc viết lại toàn bộ part). Hằng số, không phải quyết định kiến trúc — chỉnh sau khi
  đo tay.
- **`error.clickhouseHeterogeneousInsert` (D7) có thể chưa từng xảy ra từ UI hiện tại** nếu form
  insert luôn gửi đủ mọi cột cho mọi dòng — nếu vậy nhánh lỗi này chỉ là phòng thủ, không có test
  tay nào chạm tới được; chấp nhận, vẫn đúng để giữ trong code hơn là im lặng vi phạm hợp đồng.

## Những gì để lại

- **DDL** (create/rename/drop table, column, index, database) — phase riêng, cần
  `clickhouse/editing.ts` thật, dialog Structure tab, và một câu trả lời cho "ALTER TABLE của
  ClickHouse xoay quanh table engine chứ không đơn giản như MySQL" mà plan v1 đã né.
- **Dump/restore** — phase riêng, D10 của plan v1 vẫn còn nguyên.
- **Mở Query tab cho DML** (`INSERT`/`UPDATE`/`DELETE`/`TRUNCATE` gõ tay) — nếu làm, đây là lúc
  `guard.ts::writingStatements` mới thật sự cần tổng quát hoá sang tập verb thay vì boolean, đúng
  như bản phân tích ban đầu đã cân nhắc rồi bỏ ở D6. Không làm trước khi có nhu cầu thật.
- **Cancel cho mutation đang chờ (D4)** — timeout hiện chỉ dừng chờ phía client, mutation vẫn chạy
  nền trên server. `KILL MUTATION` là có thật trên ClickHouse nhưng cần tracking `mutation_id` đã
  có sẵn ở D4 — nối vào nút Cancel là việc nhỏ của một bản sau, không phải phase này.
