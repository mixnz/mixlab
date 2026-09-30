---
status: implemented
date: 2026-09-04
---

# ClickHouse: DDL (database, table, column)

Ngày: 2026-09-04

## Mục tiêu

ClickHouse (`6f72375`/`0127031` v1 chỉ đọc, `8797832` thêm ghi dòng qua lưới) vẫn đóng ba mảng còn
lại của CHANGELOG: DDL, dump/restore, Query tab DML. Spec này mở **DDL** cho database, table và
column — không đụng index (xem
[`2026-09-04-clickhouse-index-ddl-design.md`](2026-09-04-clickhouse-index-ddl-design.md), làm sau),
không đụng dump/restore
([`2026-09-04-clickhouse-dump-restore-design.md`](2026-09-04-clickhouse-dump-restore-design.md)),
không đụng Query tab
([`2026-09-04-clickhouse-query-dml-design.md`](2026-09-04-clickhouse-query-dml-design.md)).

Sau khi làm xong: mở một kết nối ClickHouse, Structure tab và "Add table" không còn xám. Tạo
database mới, tạo bảng mới (chọn engine từ danh sách MergeTree family), đổi tên/xoá bảng, xoá
database — tất cả chạy thật trên server. Trên Structure tab của một bảng đã có: thêm cột, sửa cột
(đổi tên/kiểu/nullable/default/comment), xoá cột — chạy thật. "Add index" trên Structure tab vẫn
không hiện gì (indexKinds rỗng). Query tab vẫn chặn `INSERT`/`UPDATE`/`DELETE`/DDL gõ tay như trước.
Dump/Restore trên sidebar vẫn xám.

## Phi mục tiêu

- **Index** (data skipping index) — spec riêng, làm sau spec này.
- **Sửa `ORDER BY`/sorting key sau khi bảng đã tạo** — `ALTER TABLE ... MODIFY ORDER BY` nặng (viết
  lại toàn bộ part), thuộc phạm vi spec index ở trên, không phải spec này.
- **Đổi Engine sau khi tạo bảng** — ClickHouse không có `ALTER TABLE ... ENGINE`, chọn sai phải tạo
  lại bảng. Dialog chỉ cảnh báo, không có gì để "sửa" ở đây.
- **Dump/restore, Query tab DML/DDL gõ tay** — không đổi, xem hai spec liên quan ở trên.
- **`MATERIALIZED`/`ALIAS` default, codec cột, TTL cột** — chỉ hỗ trợ `DEFAULT` thường, cùng tinh
  thần D8 của spec row-writes (không cố phủ hết mọi khả năng của ClickHouse ở một phase).
- **Tạo cột kiểu `Array`/`Map`/`Tuple`/... qua dialog** — vẫn ngoài whitelist D7 của plan v1; cột
  kiểu đó chỉ sửa được nếu đã tồn tại từ trước (qua "unknown type" entry của `ColumnDialog`), không
  tạo mới được từ dropdown.

## Hiện trạng

| Chỗ | Điều spec dựa vào |
| --- | --- |
| [`src-tauri/src/modules/db/drivers/clickhouse.rs`](../../apps/desktop/src-tauri/src/modules/db/drivers/clickhouse.rs) | `structure_columns` (đã đọc `system.columns`, giữ nguyên `dataType` kể cả wrapper `Nullable(...)`), `run_mutation_and_wait`/`matched_count` (D4/D3 của spec row-writes, có sẵn), `quote_ident`, `qualified` |
| [`src-tauri/src/modules/db/drivers/postgres_ddl.rs:410-524`](../../apps/desktop/src-tauri/src/modules/db/drivers/postgres_ddl.rs) | `modify_column`: đọc cột hiện tại, chỉ phát ra câu lệnh cho phần thực sự đổi, `RENAME COLUMN` trước để các câu sau dùng tên mới — khuôn mẫu cho `modify_column` của ClickHouse (D5) |
| [`src/modules/db/components/TableDialog/TableDialog.tsx`](../../apps/desktop/src/modules/db/components/TableDialog/TableDialog.tsx) | `extraFields` (điểm mở rộng có sẵn, Postgres/MySQL đã dùng cho collation), ternary theo `kind` đã có tiền lệ (dòng 41, hint riêng cho Postgres) |
| [`src/modules/db/components/ColumnDialog/ColumnDialog.tsx`](../../apps/desktop/src/modules/db/components/ColumnDialog/ColumnDialog.tsx) | `parseType`/`composeType`: tách/dựng type từ một cặp `(...)` duy nhất — không biết khái niệm type bọc trong type |
| [`src/modules/db/components/DatabaseActions/DatabaseActions.tsx`](../../apps/desktop/src/modules/db/components/DatabaseActions/DatabaseActions.tsx) | Một prop `disabled` duy nhất gác cả ba nút Dump/Restore/Drop; comment dòng 66-69 xác nhận Dump/Restore của ClickHouse hiện đóng *nhờ* `readOnly` từ `dialect.writable`, không nhờ cơ chế nào khác |
| [`src/modules/db/DbTab.tsx:767-768`](../../apps/desktop/src/modules/db/DbTab.tsx) | Một `readOnly` (từ `writable`) rẽ vào `SqlWorkspace`, dùng chung cho `TableStructure`, `QueryEditor`, sidebar "Add table", `DatabaseActions` — điểm cần tách thêm (D2) |
| [`src/modules/db/components/QueryEditor/QueryEditor.tsx:428-432`](../../apps/desktop/src/modules/db/components/QueryEditor/QueryEditor.tsx) | `if (readOnly) { block writing statements }` — không `readOnly` thì **không chặn gì**, không phân biệt DDL/DML. Xác nhận: không thể bật `writable` mà không vô tình mở Query tab |
| plan v1 `2026-09-04-clickhouse-db-kind.md` (local, never committed) | D7 (whitelist decodable — cũng là whitelist tạo cột mới), D9 (không có collation) |
| `docs/specs/2026-09-04-clickhouse-row-writes-design.md` | D3/D4 (`matched_count`/`run_mutation_and_wait` — có thể cần tái dùng cho `modify_column`, xem D6), D6 (tiền lệ tách cờ `rowsWritable` khỏi `writable` — spec này lặp lại đúng cách đó một lần nữa cho DDL) |

## Quyết định đã chốt

**D1 — Cờ mới `SqlDialect.ddlWritable: boolean`, tách khỏi `writable` — lặp lại đúng cách D6 của
spec row-writes đã tách `rowsWritable`.**

Phát hiện khi brainstorm: `writable` hiện là *một* cờ gác cùng lúc ba thứ độc lập — Structure tab
(`TableStructure`/"Add table"), Query tab (`QueryEditor`'s `readOnly` chặn mọi statement ghi, không
phân biệt DDL/DML), và Dump/Restore (`DatabaseActions`, theo đúng comment trong chính file đó).
Bật `writable` cho ClickHouse để mở Structure tab sẽ **vô tình mở luôn** hai spec đã cố tình để
sau (Query tab DML, dump/restore) — vi phạm chính ranh giới vừa vạch ra ở hai file spec kia.

Sửa: `ddlWritable` mới, riêng cho Structure tab + tạo/đổi tên/xoá bảng + tạo/xoá database + nút
Drop của `DatabaseActions`. `writable` giữ nguyên tên, thu hẹp nghĩa lại còn "Query tab + Dump/
Restore" (doc comment sửa lại) — vẫn `false` cho ClickHouse, không đổi hành vi hai chỗ đó.
MySQL/Postgres/SQLite: `ddlWritable: true` (giá trị y hệt `writable` hiện có ở đó, không đổi hành
vi). ClickHouse: `ddlWritable: true` (thay đổi duy nhất), `writable` vẫn `false`.

`DbTab.tsx` tính thêm `schemaReadOnly = (activeSavedConnection?.readOnly ?? false) ||
!engine.dialect.ddlWritable`, truyền vào `SqlWorkspace` cạnh `readOnly`/`dataReadOnly` đã có.
`SqlWorkspace.tsx` nhận prop `schemaReadOnly`, chuyển vào `TableStructure` và nút "Add table" của
sidebar thay cho `readOnly` cũ; `QueryEditor` giữ nguyên `readOnly` (từ `writable`, không đổi).
`DatabaseActions` cần thêm một prop mới (ví dụ `schemaDisabled`) chỉ áp cho nút Drop; Dump/Restore
tiếp tục dùng `disabled` cũ (từ `readOnly`/`writable`) — giữ đúng bất biến mà comment dòng 66-69
của file đó đang mô tả.

**D2 — Tạo bảng: dialog thêm dropdown Engine, không thêm ô ORDER BY.**

Bảng vẫn tạo gần rỗng — một cột placeholder `id UInt64` (không nullable, không default) — đúng
pattern "tạo tối thiểu, thêm cột thật ở Structure tab" ba engine kia đang dùng. Vì lúc `CREATE
TABLE` chạy chỉ có đúng cột đó, một ô ORDER BY tự do sẽ chỉ nhận được `tuple()` hoặc `id` là hợp lệ
— bất kỳ tên cột thật nào gõ vào đều lỗi "column doesn't exist" vì cột chưa tồn tại. Bỏ hẳn ô đó,
luôn tạo `ENGINE = <đã chọn> ORDER BY tuple()`. Đặt `ORDER BY` thật là việc của spec index (D2 ở
đó, sau khi bảng đã có cột thật).

Engine dropdown: `MergeTree`, `ReplacingMergeTree`, `SummingMergeTree`, `AggregatingMergeTree` —
**bốn**, không phải sáu. `CollapsingMergeTree` và `VersionedCollapsingMergeTree` cùng họ MergeTree
nhưng **bắt buộc có tham số** trỏ tới một cột `sign` (và `version`) chưa tồn tại lúc bảng
placeholder ra đời; server từ chối thẳng `Code: 42 ... requires 1 parameter` — xác minh trên
`clickhouse-test-server` 2026-09-04. Vẫn đúng phạm vi D "Không sửa engine không phải MergeTree
family" của spec row-writes áp dụng ngược lại cho *tạo mới*: không tạo
`Distributed`/`Kafka`/`View`/... qua dialog này. Dialog có dòng cảnh báo "không đổi được sau khi
tạo" — cùng cách `TableDialog.tsx` đã cảnh báo về collation.

Thêm vào `extraFields` của `TableDialog.tsx`, gated bằng `kind === "clickhouse"` trực tiếp (không
thêm cờ `SqlEditing` riêng — chỉ một dialect có trường này, khác collation vốn dùng chung cơ chế
`objectCollation` cho MySQL). Tiền lệ: dòng 41 của file đó đã dùng ternary theo `kind` cho hint.

`SqlApi.createTable` thêm tham số cuối `engine: string | null` — `null`/bỏ qua với ba dialect kia,
giống cách `collation` đã optional-theo-dialect từ trước.

**D3 — Rename/drop table, create/drop database: DDL đơn giản, không có gì đặc biệt của
ClickHouse.**

`RENAME TABLE old TO new`, `DROP TABLE`, `CREATE DATABASE name`, `DROP DATABASE name` — không
transaction cần thiết (mỗi câu tự nó là toàn vẹn). `collation` của `createDatabase` tiếp tục bị bỏ
qua (D9 của plan v1 — ClickHouse không có collation).

**D4 — `addColumn`: `ALTER TABLE t ADD COLUMN name type [DEFAULT expr] [COMMENT '...']`, luôn nối
cuối bảng.**

`SqlEditing.columnPosition: false` cho ClickHouse — dù `ADD COLUMN ... AFTER` có tồn tại,
`MODIFY COLUMN` (D5) thì không di chuyển được vị trí, và cờ hiện tại dùng chung nhị phân cho cả hai
thao tác. Postgres/SQLite cũng đã `false`. Cột mới luôn nối cuối; đơn giản hơn cho phase 1, không
mất khả năng gì đã hứa ở nơi khác.

**D5 — `modifyColumn`: diff-based, theo đúng khuôn `postgres_ddl.rs::modify_column` — đọc cột hiện
tại, chỉ phát câu lệnh cho phần thực sự đổi, RENAME trước.**

```
current = đọc từ structure_columns (đã có)
statements = []
nếu spec.name != current.name:
    statements.push(RENAME COLUMN <current.name> TO <spec.name>)
nếu spec.dataType/defaultValue/comment khác current (bất kỳ cái nào):
    -- dataType đã chứa wrapper Nullable(...) nếu có (D7 bọc ở frontend trước khi gửi),
       nên so dataType là đủ để bắt luôn thay đổi nullable, không cần so field riêng
    statements.push(MODIFY COLUMN <spec.name> <type đầy đủ> [DEFAULT <expr>] [COMMENT '<c>'])
```

Khác Postgres: `MODIFY COLUMN` của ClickHouse nhận một khai báo cột trọn vẹn trong một mệnh đề,
không cần tách thành nhiều `ALTER COLUMN SET ...` như Postgres phải làm. Không có gì đổi → không
câu lệnh nào cả (giữ đúng lý do Postgres diff: một lần lưu comment không nên trả giá bằng một lần
đổi kiểu).

**Rủi ro chưa giải được (không transaction):** `RENAME` thành công rồi `MODIFY` thất bại — cột đã
đổi tên, thuộc tính khác chưa đổi, không rollback. Cùng bản chất rủi ro đã chấp nhận ở D4 của spec
row-writes.

**Hai câu lệnh tách rời là bắt buộc, không phải lựa chọn.** Gộp `RENAME COLUMN x TO y, MODIFY
COLUMN y ...` vào một `ALTER` bị server từ chối thẳng: `Code: 48 ... Cannot rename and modify the
same column 'y' in a single ALTER query (NOT_IMPLEMENTED)`. Vì vậy rủi ro không-rollback ở trên
không có cách nào né.

**Đã xác minh (2026-09-04, `clickhouse-test-server` 26.8.2.7, bảng MergeTree 43 triệu dòng):
`MODIFY COLUMN` đổi kiểu *có* tạo mutation, nhưng lệnh `ALTER` tự chờ mutation xong rồi mới trả
lời — `modify_column` KHÔNG cần `run_mutation_and_wait`, chạy như `execute_check` thường.**

- Đổi kiểu (`UInt32`→`UInt64`→`String`→`UInt64`→`Int128`) tạo dòng mới trong `system.mutations`,
  nhưng ngay khi HTTP request trả về thì `is_done = 1`, `parts_to_do = 0` và dữ liệu đã chuyển
  xong. Lệnh chặn 2.7–3.4 s cho 43 triệu dòng (so với 0.16 s của thay đổi metadata thuần).
- Stack trace của một lần đổi kiểu lỗi cho thấy đúng đường đi đồng bộ:
  `StorageMergeTree::alter` → `StorageMergeTree::waitForMutation` → `checkMutationStatus`.
- `mutations_sync` (mặc định `0` = không chờ) **không** áp cho `MODIFY COLUMN`: mô tả của chính
  setting đó chỉ liệt kê `UPDATE|DELETE|MATERIALIZE INDEX|MATERIALIZE PROJECTION|MATERIALIZE
  COLUMN|MATERIALIZE STATISTICS`. `alter_sync` (mặc định `1` = chờ) chỉ áp cho bảng
  `Replicated`/`Shared`.
- Chỉ đổi comment/default (không đổi kiểu): 0.16 s, **không** sinh mutation nào — metadata thuần,
  đúng như dự đoán. `RENAME COLUMN` cũng vậy (0.25 s, không mutation).

**Rủi ro mới phát hiện trong lúc xác minh — đổi kiểu thất bại vì dữ liệu làm bảng KHÔNG ĐỌC ĐƯỢC.**
Đổi một cột `String` chứa `'a'` sang `UInt64`: lệnh trả về lỗi ngay (0.2 s, `Code: 341 ...
UNFINISHED`, kèm lý do `CANNOT_PARSE_TEXT`) — nhưng **metadata đã đổi rồi**: `system.columns` báo
`note UInt64`, mutation nằm lại `is_done = 0, parts_to_do = 2`, và mọi `SELECT` trên bảng từ đó
đều lỗi `Cannot parse string 'a' as UInt64`. Bảng hỏng cho tới khi người dùng đổi kiểu ngược lại
(`MODIFY COLUMN note String` — 9.1 s, mutation hỏng biến mất, bảng đọc lại được) hoặc
`KILL MUTATION`.

Hệ quả cho thiết kế: khi `modify_column` trả lỗi, **không được để người dùng nghĩ là "không có gì
xảy ra"**. Thông báo lỗi phải nói rõ bảng đang ở trạng thái hỏng và cách thoát (đổi kiểu về lại
kiểu cũ). Cần một khoá i18n riêng cho trường hợp này — đây là khoá mới duy nhất spec cần thêm.

**D6 — `dropColumn`: `ALTER TABLE t DROP COLUMN name`, không gì đặc biệt.**

**D7 — Lớp bóc/bọc `Nullable(T)` riêng cho ClickHouse, không sửa `parseType`/`composeType` dùng
chung.**

`Nullable(T)` không phải nullability tách rời như MySQL/Postgres (`int NULL` vs `int NOT NULL`) —
nó nằm ngay trong cách viết type (`Nullable(UInt64)` vs `UInt64`). `structure_columns` (đã build)
trả `dataType` **nguyên văn kể cả wrapper** — một cột nullable có `dataType: "Nullable(UInt64)"`.
`ColumnDialog.tsx`'s `parseType()` tách type ở dấu `(` **đầu tiên** để tìm tên + argument, giả định
đúng một cặp ngoặc — với `"Nullable(UInt64)"` (hay tệ hơn, `"Nullable(Decimal(10,2))"` — hai cặp
ngoặc lồng nhau), điều này parse sai: `typeName` không khớp entry nào của whitelist `columnTypes`,
rơi vào nhánh "type lạ", dropdown hiện nguyên chuỗi thô thay vì `UInt64` đã chọn sẵn với checkbox
Nullable bật.

Sửa: không đụng `parseType`/`composeType` (dùng chung, ảnh hưởng 3 dialect kia). Thêm bước bóc/bọc
mỏng, chỉ chạy khi `kind === "clickhouse"`, ở đúng hai chỗ chuyển đổi hiện có:
- Đọc `SqlStructureColumn.dataType` để dựng `Draft` ban đầu: nếu bắt đầu bằng `"Nullable("` và kết
  thúc bằng `")"`, bóc lớp đó ra trước khi đưa vào `parseType` (nullable đã có sẵn từ
  `SqlStructureColumn.nullable`, không cần suy lại).
- Dựng `SqlColumnSpec.dataType` từ `Draft` (sau `composeType`): nếu `draft.nullable`, bọc
  `Nullable(...)` quanh kết quả trước khi gửi đi.

**Bổ sung sau khi triển khai — `parseType`/`composeType` vẫn phải sửa, vì một lý do khác:** tên kiểu
của ClickHouse **phân biệt hoa/thường** (`uint64` bị từ chối `Code: 50 ... Unknown data type family`,
xác minh 2026-09-04), trong khi `parseType` hạ chữ thường `typeName` vô điều kiện và `typeSpec` so
`type.name === name.toLowerCase()`. Hai điều đó cộng lại làm whitelist ClickHouse **không bao giờ
khớp** và mọi cột tạo/sửa qua dialog đều sinh SQL server không nhận. Sửa ở hai chỗ dùng chung, an
toàn cho ba engine kia (danh sách của chúng vốn toàn chữ thường):
- `typeSpec` so không phân biệt hoa/thường cả hai vế.
- `parseType` trả về tên đúng như danh sách của engine viết (`typeSpec(types, raw)?.name ?? raw`),
  thay vì hạ chữ thường — một kiểu không có trong danh sách giữ nguyên cách viết nó đến.

**D8 — `SqlEditing` thêm `autoIncrement: boolean` — checkbox "Auto increment" hiện không bị gate
bởi cờ nào, luôn hiện cho mọi dialect.**

Đúng cho MySQL/Postgres/SQLite (cả ba đều có khái niệm tương đương: `AUTO_INCREMENT`, `GENERATED
...IDENTITY`, `AUTOINCREMENT`). ClickHouse là kind đầu tiên không có gì tương đương — cần
`autoIncrement: false` để ẩn hẳn checkbox, cùng mẫu với `onUpdateCurrentTimestamp` đã có sẵn
(`offers.onUpdateCurrentTimestamp && (...)` ở `ColumnDialog.tsx:427`). MySQL/Postgres/SQLite:
`autoIncrement: true` (không đổi hành vi hiện tại — checkbox tiếp tục hiện y hệt trước).

**D9 — `SqlEditing.markExpressionDefaults: true` cho ClickHouse.**

`system.columns.default_expression` có cùng kiểu mơ hồ literal-vs-expression MySQL gặp phải
(`'active'` khác `now()`) — dùng đúng cơ chế đã có (D8 của spec row-writes: giá trị ghi gửi nguyên
text, không ép kiểu ở tầng này). **Chưa test tay** việc ClickHouse phân biệt hai trường hợp này
chính xác đến đâu — ghi vào Kiểm thử.

**D10 — `columnTypes`: whitelist decode-được của D7 plan v1, không thêm gì mới.**

`UInt*`, `Int*`, `Float32/64`, `String`, `FixedString`, `Date`, `Date32`, `DateTime`, `DateTime64`,
`Decimal*`, `UUID`, `Enum8/16`, `Bool`. Xác nhận qua đọc `ColumnDialog.tsx`: dropdown loại cột cho
cột **mới** chỉ lấy từ `columnTypes` (cột kiểu `Array`/`Map` không tạo mới được qua dialog, đúng
tinh thần D7 plan v1 — "unknown type" entry chỉ giữ nguyên type đã có khi *sửa* một cột đã tồn tại
kiểu đó, không cho phép chọn khi tạo mới).

## Backend — file đổi

```
src-tauri/src/modules/db/drivers/clickhouse_ddl.rs   (module mới, theo khuôn postgres_ddl.rs —
  clickhouse.rs đã 1463 dòng và chỉ lo phần đọc)
src-tauri/src/modules/db/drivers/clickhouse.rs
  qualified/quote_literal/structure_columns nới lên pub(super); structure_columns đọc default
  phân biệt literal với expression (read_default)
  + pub async fn create_table(conn, database, table, engine: &str) -> Result<(), AppError>   (D2)
  + pub async fn rename_table(conn, database, table, new_name) -> Result<(), AppError>        (D3)
  + pub async fn drop_table(conn, database, table) -> Result<(), AppError>                    (D3)
  + pub async fn create_database(conn, name) -> Result<(), AppError>                          (D3)
  + pub async fn drop_database(conn, name) -> Result<(), AppError>                             (D3)
  + pub async fn add_column(conn, database, table, spec: &ColumnSpec) -> Result<(), AppError> (D4)
  + pub async fn modify_column(conn, database, table, name, spec: &ColumnSpec)                (D5)
      -- chạy như execute_check thường: ALTER tự chờ mutation xong (đã xác minh 2026-09-04),
         KHÔNG dùng run_mutation_and_wait
  + pub async fn drop_column(conn, database, table, name) -> Result<(), AppError>              (D6)

src-tauri/src/modules/db/commands/clickhouse.rs
  + clickhouse_create_table, clickhouse_rename_table, clickhouse_drop_table,
    clickhouse_create_database, clickhouse_drop_database,
    clickhouse_add_column, clickhouse_modify_column, clickhouse_drop_column
    (chữ ký giống hệt các lệnh tương ứng ở commands/mysql.rs)

src-tauri/src/modules/mod.rs
  + tám dòng generate_handler! cho tám lệnh trên
```

## Frontend — file đổi

```
src/modules/db/sql/dialect.ts        + ddlWritable: boolean; sửa doc comment writable (D1)
src/modules/db/sql/api.ts            createTable(...) thêm tham số cuối engine: string | null (D2)
src/modules/db/mysql/dialect.ts      + ddlWritable: true
src/modules/db/postgres/dialect.ts   + ddlWritable: true
src/modules/db/sqlite/dialect.ts     + ddlWritable: true
src/modules/db/clickhouse/dialect.ts + ddlWritable: true   (writable vẫn false)
src/modules/db/mysql/editing.ts      + autoIncrement: true
src/modules/db/postgres/editing.ts   + autoIncrement: true
src/modules/db/sqlite/editing.ts     + autoIncrement: true
src/modules/db/clickhouse/editing.ts   giá trị thật thay shape rỗng (D8, D9, D10):
                                        columnTypes: whitelist D7 plan v1, unsigned: false,
                                        columnPosition: false, onUpdateCurrentTimestamp: false,
                                        objectCollation: false, markExpressionDefaults: true,
                                        autoIncrement: false, indexKinds: [], indexMethods: [],
                                        indexPrefix: false, primaryKeyName: null
src/modules/db/clickhouse/api.ts     createTable/renameTable/dropTable/createDatabase/
                                      dropDatabase/addColumn/modifyColumn/dropColumn:
                                      invoke() thật thay notSupported()
src/modules/db/DbTab.tsx             + schemaReadOnly, truyền cùng readOnly/dataReadOnly (D1)
src/modules/db/sql/SqlWorkspace.tsx  + prop schemaReadOnly; TableStructure + "Add table" dùng nó;
                                      QueryEditor giữ nguyên readOnly (D1)
src/modules/db/components/DatabaseActions/DatabaseActions.tsx
                                      + prop schemaDisabled, áp cho nút Drop; Dump/Restore giữ
                                      nguyên disabled cũ (D1)
src/modules/db/components/TableDialog/TableDialog.tsx
                                      + Engine dropdown trong extraFields khi kind === "clickhouse",
                                      dòng cảnh báo "không đổi được sau khi tạo" (D2)
src/modules/db/components/ColumnDialog/ColumnDialog.tsx
                                      + bóc/bọc Nullable(...) quanh parseType/composeType khi
                                      kind === "clickhouse" (D7)
```

## Kiểm thử

**Rust, thuần** (`cargo test`, chạy CI):

- `modify_column`'s phần dựng câu (tách khỏi phần gửi HTTP, test thuần, theo mẫu
  `postgres_ddl.rs::modify_column` đã có test tương tự): chỉ đổi tên → một `RENAME COLUMN`; chỉ
  đổi comment → một `MODIFY COLUMN` không đụng type; đổi cả tên lẫn type → hai câu, RENAME trước;
  không đổi gì → không câu lệnh nào.
- `create_table`'s phần dựng câu: đúng `ENGINE = <engine> ORDER BY tuple()`, cột placeholder
  `id UInt64`.
- Chuỗi lỗi giữ đúng khoá i18n đã đặt (`error.tableNameRequired`, `error.columnNameRequired`,
  `error.columnTypeRequired` — dùng chung với MySQL/Postgres/SQLite, không thêm khoá mới trừ khi
  phát sinh lỗi riêng của ClickHouse).

**Vitest, thuần:**

- Bóc/bọc `Nullable(...)` (D7): round-trip `"Nullable(UInt64)"` → bóc → `parseType` → `composeType`
  → bọc lại → đúng `"Nullable(UInt64)"`; `"UInt64"` (không nullable) đi qua không đổi; trường hợp
  lồng `"Nullable(Decimal(10,2))"` bóc đúng lớp ngoài, giữ nguyên `"Decimal(10,2)"` bên trong.
- `ddlWritable`/`autoIncrement` — giá trị đúng theo dialect, giống style test `hasTls` đã có.

**Bằng tay, ghi vào báo cáo cuối** (theo đúng tiền lệ, server thật `clickhouse-test-server`, throwaway
`cargo run --example`, xoá sau khi xong):

- Tạo database mới, tạo bảng với từng engine trong danh sách — xác nhận `ORDER BY tuple()` mặc
  định, cột `id` xuất hiện đúng.
- Thêm cột nullable và non-nullable của vài kiểu trong whitelist.
- ~~**Xác minh D5's rủi ro chưa giải:** sửa kiểu một cột đã có dữ liệu → kiểm `system.mutations`
  có dòng mới xuất hiện hay không (mutation bất đồng bộ) hay lệnh trả lời ngay (đồng bộ).~~
  **Đã làm 2026-09-04, kết quả trong D5: đồng bộ, không cần `run_mutation_and_wait`.**
- **Còn phải kiểm tay:** đổi kiểu thất bại vì dữ liệu (`String` chứa chữ → `UInt64`) trên một bảng
  thật qua UI — xác nhận thông báo lỗi của standalone-client nói được cho người dùng biết bảng đang hỏng và
  cách thoát (xem rủi ro mới ở D5).
- Sửa comment-only, default-only, đổi tên-only trên cùng một cột — xác nhận đúng câu lệnh tối
  thiểu được gửi (đối chiếu `system.query_log` nếu cần).
- Mở dialog sửa một cột `Nullable(UInt64)` đã có sẵn dữ liệu — xác nhận dropdown hiện đúng
  `UInt64` đã chọn, checkbox Nullable bật, không hiện "unknown type" (xác nhận D7).
- Xoá cột, đổi tên bảng, xoá bảng, xoá database.
- ~~**Xác minh D9:** tạo cột `DEFAULT 'active'` (literal) và `DEFAULT now()` (expression) — đọc lại
  `default_expression`.~~ **Đã làm 2026-09-04:** `system.columns` trả `default_kind = 'DEFAULT'`
  cho cả hai, `default_expression` là `'active'` (có nháy đơn) và `now()` (không nháy) — phân biệt
  được đúng bằng cùng heuristic MySQL đang dùng, `markExpressionDefaults: true` giữ nguyên. Còn
  phải xác nhận phần hiển thị trong lưới qua UI.
- Query tab: gõ tay `CREATE`/`INSERT`/`UPDATE` trên kết nối ClickHouse → vẫn bị `guard.ts` chặn như
  trước (xác nhận D1 không vô tình mở Query tab).
- Sidebar: nút Dump/Restore vẫn xám dù Structure tab đã mở; nút Drop database thì bấm được (xác
  nhận D1's tách `schemaDisabled` khỏi `disabled` đúng trong `DatabaseActions`).

## Rủi ro

- ~~**`MODIFY COLUMN` có thể là mutation bất đồng bộ, chưa xác minh (D5).**~~ **Đã loại bỏ
  2026-09-04:** lệnh `ALTER` tự chờ mutation xong (`StorageMergeTree::waitForMutation`), không cần
  `run_mutation_and_wait`. Chi tiết ở D5.
- **Đổi kiểu thất bại vì dữ liệu làm bảng không đọc được cho tới khi đổi kiểu ngược lại (D5).**
  Rủi ro lớn nhất còn lại của spec này, phát hiện lúc xác minh: metadata đổi trước, mutation hỏng
  nằm lại, `SELECT` lỗi. Lớp phòng thủ duy nhất là thông báo lỗi nói rõ cách thoát — cần khoá i18n
  riêng.
- **`RENAME COLUMN` thành công rồi `MODIFY COLUMN` thất bại giữa chừng, không rollback (D5).** Cùng
  bản chất đã chấp nhận ở D4 của spec row-writes — không transaction, và ClickHouse cấm hẳn việc
  gộp hai thao tác đó vào một `ALTER` (`Code: 48 NOT_IMPLEMENTED`, xác minh 2026-09-04), nên không
  có cách nào khác.
- **Engine chọn sai lúc tạo bảng không sửa được, phải tạo lại (D2).** Cảnh báo trong dialog là lớp
  phòng thủ duy nhất; chấp nhận được vì đúng bản chất ClickHouse.
- ~~**`markExpressionDefaults: true` (D9) chưa kiểm tay**~~ — **đã kiểm 2026-09-04:**
  `default_expression` phân biệt `'active'` với `now()` đúng như MySQL, rủi ro này khép lại (phần
  hiển thị trong lưới vẫn còn phải xem qua UI).
- **`Nullable(Decimal(10,2))` hay các type lồng khác** — D7 giải quyết đúng một lớp bọc ngoài cùng;
  nếu ClickHouse cho phép lồng sâu hơn (không phổ biến), bóc/bọc có thể sai — whitelist D7 plan v1
  vốn chỉ gồm scalar, rủi ro này thấp nhưng chưa loại trừ hoàn toàn.

## Những gì để lại

- **Index** (data skipping index, sửa ORDER BY sau khi tạo) —
  [`2026-09-04-clickhouse-index-ddl-design.md`](2026-09-04-clickhouse-index-ddl-design.md).
- **Dump/restore** —
  [`2026-09-04-clickhouse-dump-restore-design.md`](2026-09-04-clickhouse-dump-restore-design.md).
- **Query tab cho DML/DDL gõ tay** —
  [`2026-09-04-clickhouse-query-dml-design.md`](2026-09-04-clickhouse-query-dml-design.md).
- **Đổi Engine sau khi tạo bảng** — không có lệnh ClickHouse nào làm việc này trực tiếp; phải tạo
  bảng mới rồi chuyển dữ liệu, ngoài phạm vi một dialog Structure tab.
- **`MATERIALIZED`/`ALIAS` default, codec, TTL cột** — xem Phi mục tiêu.
