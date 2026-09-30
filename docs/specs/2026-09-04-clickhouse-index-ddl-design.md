---
status: implemented
date: 2026-09-04
---

# ClickHouse: index DDL (data skipping index + rebuild ORDER BY)

Ngày: 2026-09-04

## Mục tiêu

Tiếp theo sau [`2026-09-04-clickhouse-ddl-design.md`](2026-09-04-clickhouse-ddl-design.md) (đã merge
qua `f9910a4`) — spec đó cố tình để lại index, ghi rõ "làm sau". Spec này mở nốt phần index còn thiếu
trên Structure tab của một kết nối ClickHouse: **data skipping index** (thêm/sửa/xoá) và **đổi sorting
key** (`ORDER BY`) của một bảng đã tồn tại, qua rebuild toàn bảng.

Sau khi làm xong: Structure tab của một bảng ClickHouse có thêm panel "Skip indexes" — thêm, sửa, xoá
được, chạy thật trên server. Dòng `sorting_key` trong panel "Indexes" hiện có (đọc-only trước đây) có
nút Edit mở dialog "đổi sorting key" — chạy thật, rebuild toàn bảng bằng bảng tạm rồi hoán tên atomic.
Nút Drop trên dòng đó vẫn tắt (không có khái niệm "xoá" sorting key).

## Phi mục tiêu

- **Chọn `ORDER BY` ngay lúc `CREATE TABLE`** — trải nghiệm tốt hơn hẳn dance "tạo bảng rỗng → thêm
  cột → rebuild", nhưng thuộc phạm vi D2 của spec DDL chính đã merge (lúc đó cột thật chưa tồn tại).
  Không mở lại quyết định đó ở đây — ghi nhận là cải tiến để lại cho sau.
- **Giới hạn TYPE của skip index theo kiểu dữ liệu cột** (ví dụ ẩn `ngrambf_v1`/`tokenbf_v1` khi cột
  không phải `String`/`FixedString`) — server tự từ chối với lỗi rõ ràng nếu chọn sai, dialog không
  lặp lại việc đó, giống cách app không tiền-kiểm hợp lệ biểu thức SQL ở nơi khác.
- **Kiểm dung lượng đĩa còn trống trước khi rebuild** — đáng làm ở một phase sau, nhưng v1 không có
  cách rẻ nào đọc quota đĩa qua ClickHouse HTTP mà không thêm một lớp mới; để lại.
- **Progress bar / huỷ giữa chừng khi rebuild** — `INSERT INTO ... SELECT` qua HTTP là một request
  đồng bộ, không có kênh nào báo tiến độ; huỷ giữa chừng cần `query_id` + `KILL QUERY`, chưa có ở v1
  (xem `SqlDialect.cancellable` — đã `false` cho ClickHouse, lý do y hệt).
- **`ON CLUSTER`, bảng `Replicated*`** — rebuild chỉ chạy trên bảng thuộc 4 engine không-replicated đã
  whitelist ở D2 của spec DDL chính; xem D11 dưới.

## Hiện trạng

| Chỗ | Điều spec dựa vào |
| --- | --- |
| [`src-tauri/src/modules/db/drivers/clickhouse.rs:976-1010`](../../apps/desktop/src-tauri/src/modules/db/drivers/clickhouse.rs) | `table_structure`: `indexes` hiện chỉ có đúng một dòng giả `sorting_key` (đọc từ cột có `key == "PRI"`), comment tại đó đã ghi rõ "data-skipping indices... v1 leaves them out" — chính là gap spec này lấp |
| [`src/modules/db/components/IndexDialog/IndexDialog.tsx`](../../apps/desktop/src/modules/db/components/IndexDialog/IndexDialog.tsx) | Docstring: "MySQL cannot alter an index in place, so an edit is a drop and a rebuild" — tiền lệ cho D4 (skip index cũng vậy, đã verify server từ chối `MODIFY INDEX`) |
| [`src/modules/db/components/TableStructure/TableStructure.tsx:684-886`](../../apps/desktop/src/modules/db/components/TableStructure/TableStructure.tsx) | Panel "Indexes" hiện có, nút Add hiện đang **không bị chặn** cho ClickHouse (`disabled={noWrites \|\| columns.length === 0}`, không xét `indexKinds`) — mở `IndexDialog` với `kindOptions` rỗng, submit gọi `api.addIndex` → `notSupported()`. Gap có sẵn từ trước, sửa luôn ở D5 |
| [`src/modules/db/clickhouse/api.ts:129-131`](../../apps/desktop/src/modules/db/clickhouse/api.ts) | `addIndex`/`modifyIndex`/`dropIndex` là `notSupported()` — **giữ nguyên vĩnh viễn**, ClickHouse không có khái niệm index tra cứu. Bốn API mới của spec này (`addSkipIndex`/`modifySkipIndex`/`dropSkipIndex`/`rebuildOrderBy`) là API khác, không thay thế ba cái này |
| [`src/components/ConfirmDialog/ConfirmDialog.tsx`](../../apps/desktop/src/components/ConfirmDialog/ConfirmDialog.tsx) | `children` slot có sẵn cho "options that change what confirming will do" — không dùng được thẳng cho D6 vì thiếu `confirmDisabled`; `OrderByDialog` tự dựng thay vì mở rộng component này (xem D6) |
| `docs/specs/2026-09-04-clickhouse-ddl-design.md` | D1 (`ddlWritable` tách khỏi `writable`, đã mở Structure tab), D2 (4 engine whitelist lúc tạo bảng: `MergeTree`/`ReplacingMergeTree`/`SummingMergeTree`/`AggregatingMergeTree`, không replicated — D11 dưới áp lại whitelist này cho *rebuild* trên bảng *đã có*) |

**Đã verify trên `clickhouse-test-server` (26.8.2.7), tất cả throwaway, xoá sau khi xong:**

- `ALTER TABLE t MODIFY ORDER BY (...)` **chỉ chạy được khi đi kèm `ADD COLUMN` trong cùng một câu,
  và chỉ chấp nhận cột mới thêm** — tham chiếu bất kỳ cột nào đã tồn tại từ trước đều bị từ chối:
  `Code: 36 ... Existing column X is used in the expression that was added to the sorting key. You
  can add expressions that use only the newly added columns.` Không có cách nào dùng ALTER để đặt
  sorting key từ các cột đã có trong bảng — đây là lý do duy nhất khiến hướng "rebuild toàn bảng" là
  hướng còn lại, không phải một trong hai lựa chọn.
- `SHOW CREATE TABLE` luôn đặt `ORDER BY ...` trên một dòng riêng của chính nó (`ORDER BY tuple()`
  khi rỗng, `ORDER BY (a, b)` khi có key) — `PARTITION BY`, `TTL`, `SETTINGS`, `COMMENT` mỗi cái một
  dòng riêng bao quanh, và index phụ (`INDEX name expr TYPE ... GRANULARITY n`) nằm ngay trong khối
  cột. Một `PRIMARY KEY (...)` khai riêng (khác `ORDER BY`) cũng là dòng riêng, đứng trước `ORDER BY`.
- `EXCHANGE TABLES a AND b` chạy atomic trên database `<old>_agent_test` (engine `Atomic`, mặc định
  của ClickHouse hiện đại) — verify bằng cách tráo hai bảng có dữ liệu khác nhau, dữ liệu đi đúng
  theo tên sau khi tráo.
- `ADD INDEX ... GRANULARITY` **không bắt buộc** — bỏ qua thì server tự áp `GRANULARITY 1`. Luôn gửi
  tường minh để dialog và server không lệch giá trị mặc định.
- `MODIFY INDEX ... TYPE ...` là **lỗi cú pháp** (`Expected one of: STATISTICS, PROJECTION,
  CONSTRAINT, COLUMN, ORDER BY, ...` — `INDEX` không nằm trong danh sách `MODIFY` chấp nhận).
- `ADD INDEX ... COMMENT '...'` cũng là **lỗi cú pháp** — skip index không có khái niệm comment.
  `system.data_skipping_indices` xác nhận: không có cột `comment`.
- Số tham số đúng của từng TYPE (khác với suy đoán ban đầu ở một chỗ): `set(N)` 1 số, `bloom_filter`
  0 hoặc 1 số (mặc định server `0.025` khi bỏ trống), `ngrambf_v1(n, size, hashes, seed)` **4** số,
  `tokenbf_v1(size, hashes, seed)` **3** số (không có `n` — khác `ngrambf_v1`, xác nhận bằng lỗi
  `tokenbf index must have exactly 3 arguments` khi thử gửi 4).
- `system.data_skipping_indices` có cột `type` (tên loại trần) và `type_full` (chữ đầy đủ kèm tham
  số, ví dụ `"ngrambf_v1(3, 256, 2, 0)"`) — đọc `type_full` để tách tên + args hiển thị lại.
- `system.tables.engine` đọc được rẻ (`SELECT engine FROM system.tables WHERE ...`) — dùng cho D11.

## Quyết định đã chốt

**D1 — Hai tính năng độc lập, không đụng `SqlIndexKind`/`IndexDialog`/`SqlEditing` dùng chung; gate
thẳng bằng `dialect.kind === "clickhouse"`, không thêm cờ dialect mới.**

Hình dạng dữ liệu khác hẳn: index tra cứu MySQL/Postgres có kind/method/prefixLength; skip index có
expr tự do + TYPE(args) + GRANULARITY bắt buộc, không unique/primary/comment; sorting key là một
danh sách cột *có thứ tự*, sửa bằng rebuild chứ không phải ALTER. `SqlEditing`/`SqlDialect` "chỉ nên
có một câu hỏi khi hai engine trả lời khác nhau" (đúng comment sẵn có ở `dialect.ts`) — không engine
nào khác có khái niệm skip index hay rebuild sorting key để so sánh, nên không thêm gì vào đó. Panel
"Skip indexes" và dialog "đổi sorting key" chỉ render khi `kind === "clickhouse"`, giống cách
`TableDialog.tsx`'s Engine dropdown đã gate trực tiếp bằng `kind`.

`addIndex`/`modifyIndex`/`dropIndex`/`indexKinds` giữ nguyên như đã merge — vĩnh viễn rỗng cho
ClickHouse, không phải chỗ để nhét skip index vào.

**D2 — Data model mới, tách khỏi `SqlTableIndex`.**

```
SqlSkipIndex { name: string; expr: string; indexType: string; args: string[]; granularity: number }
SqlSkipIndexSpec  — cùng shape, dùng cho add/modify
```

`SqlTableStructure` thêm hai field, cả hai rỗng/null cho ba dialect kia (đúng pattern `indexKinds: []`
đã có):
- `skipIndexes: SqlSkipIndex[]`
- `engine: string | null` — chỉ ClickHouse điền, dùng cho D11.

Backend: `clickhouse.rs::table_structure` thêm đọc `system.data_skipping_indices` (`name, type,
type_full, expr, granularity`, lọc `database`/`table`) và `system.tables.engine`; tách `type_full`
thành `(indexType, args)` bằng cùng cách `parseType`-kiểu-thô (tên trước dấu `(` đầu, phần trong tách
theo dấu phẩy — không cần xử lý ngoặc lồng như `Decimal(10,2)` vì không TYPE nào của skip index lồng
ngoặc).

**D3 — `SkipIndexDialog` (component mới): name, expr (text tự do, không phải picker cột), TYPE
dropdown, các ô tham số theo TYPE, GRANULARITY.**

Whitelist TYPE với đúng số tham số đã verify — `src/modules/db/clickhouse/skipIndexTypes.ts` (file
mới, không tái dùng `SqlTypeSpec` vì tham số ở đây luôn là số, không phải type-trong-type):

```
minmax        — không tham số
set           — 1 số nguyên (max_rows, 0 = không giới hạn)
bloom_filter  — 0-1 số thực (false positive rate), gợi ý mặc định 0.025 khi để trống
ngrambf_v1    — 4 số nguyên (n, size_of_bloom_filter_bytes, number_of_hash_functions, random_seed)
tokenbf_v1    — 3 số nguyên (size_of_bloom_filter_bytes, number_of_hash_functions, random_seed)
```

`expr` là text tự do vì dùng thật phổ biến là một biểu thức chứ không phải tên cột trần — ví dụ
`lower(note)` cho bloom filter không phân biệt hoa thường trên cột `String`; ép chọn cột từ dropdown
sẽ mất khả năng này.

GRANULARITY: ô số, mặc định `1` (đúng giá trị server tự áp khi bỏ trống — luôn gửi tường minh, không
để trống bao giờ). Kèm một dòng hint: GRANULARITY đếm theo *granule* (mỗi granule mặc định 8192 dòng
theo `index_granularity` của bảng), không phải theo số dòng trực tiếp — nhầm lẫn phổ biến.

**D4 — Sửa skip index = xoá rồi thêm lại, không có `modify` thật ở tầng SQL.**

Verify: `MODIFY INDEX ... TYPE ...` là lỗi cú pháp — ClickHouse không có. Backend
`modify_skip_index(conn, database, table, old_name, spec)` chạy `DROP INDEX old_name` rồi `ADD INDEX
...` bên trong cùng một hàm — đúng khuôn hai câu lệnh tách rời mà `modify_column` (spec DDL chính,
D5) và tiền lệ MySQL index (`IndexDialog.tsx`) đã dùng. Rủi ro không-rollback giữa hai câu — cùng bản
chất đã chấp nhận ở những chỗ đó, không giải lại ở đây.

Không có `comment` trên `SqlSkipIndex`/dialog — verify `ADD INDEX ... COMMENT` là lỗi cú pháp.

**D5 — Panel "Skip indexes" mới, tách khỏi panel "Indexes" hiện có. Sửa panel "Indexes" cho
ClickHouse: nút Add tắt hẳn, nút Edit trên dòng `sorting_key` mở `OrderByDialog` thay vì
`IndexDialog`, nút Drop trên dòng đó tắt.**

Panel "Indexes" tiếp tục chỉ hiện đúng một dòng `sorting_key` cho ClickHouse như đã có — không gộp
skip index vào đó, vì cột hiển thị khác hẳn (index tra cứu có method/prefixLength; skip index có
expr/type-args/granularity, không có unique/primary).

`TableStructure.tsx` hiện tại cho nút Add của panel "Indexes" chỉ xét `noWrites ||
columns.length === 0`, không xét `dialect.editing.indexKinds` — với ClickHouse (`indexKinds: []`)
nút vẫn bấm được, mở `IndexDialog` rỗng, submit lỗi `notSupported`. Sửa: thêm điều kiện
`offers.indexKinds.length === 0` vào `disabled` của nút Add — sửa chung cho mọi dialect có
`indexKinds` rỗng, không riêng ClickHouse (dù hiện chỉ ClickHouse rơi vào trường hợp đó).

Điều kiện mở `OrderByDialog` thay cho `IndexDialog`: `dialect.kind === "clickhouse" &&
index.indexType === "sorting_key"` (chuỗi cố định backend đã đặt sẵn, không đổi). Nút Drop trên dòng
đó: thêm `|| (dialect.kind === "clickhouse" && index.indexType === "sorting_key")` vào `disabled`.

**D6 — `OrderByDialog` (component mới): danh sách cột có thứ tự (add/remove, không prefixLength),
cảnh báo kèm số dòng ước lượng, gõ lại đúng tên bảng để mở khoá nút Confirm.**

Không dùng `ConfirmDialog` — cần thêm picker cột có thứ tự, không phải chỉ message + hai nút.
Dựng riêng, theo khuôn `IndexDialog`'s danh sách cột (Select mỗi dòng, nút thêm/bớt) nhưng bỏ
`prefixLength` (ClickHouse không có khái niệm đó cho sorting key).

Trước khi cho sửa: gọi `SqlApi.rowCount(connectionId, database, table)` (method mới, `notSupported()`
cho ba dialect kia, thật cho ClickHouse — `SELECT count() FROM db.t`) để hiện "Bảng có N dòng — thao
tác này copy toàn bộ N dòng, có thể mất nhiều phút với bảng lớn" trong cảnh báo. Đọc lỗi thì bỏ qua
im lặng, không hiện số — không đáng chặn cả dialog vì một con số tiện ích.

Nút Confirm bị khoá (`disabled`) cho tới khi ô nhập khớp *chính xác* (phân biệt hoa/thường, đã trim)
tên bảng đang sửa — mức cảnh báo cao nhất hiện có trong app, xứng đáng vì thao tác nặng và không thể
rollback nếu lỗi giữa chừng (xem D8). `Modal` mở với `locked={saving}` suốt quá trình, giống
`IndexDialog`, để không đóng dialog dở dang khi request đang chạy.

**D7 — `rebuildOrderBy`: chuỗi backend duy nhất, không transaction, có kiểm tra trước khi tráo tên.**

```
current_ddl = SHOW CREATE TABLE db.table
temp_name   = "{table}__<old>_rebuild_{unix_millis}"   -- có timestamp, không bao giờ đụng
                                                         -- bảng tạm còn sót từ lần chạy trước
new_ddl     = current_ddl với dòng "ORDER BY ..." thay bằng "ORDER BY (col1, col2, ...)"
              (hoặc "ORDER BY tuple()" nếu danh sách rỗng), và tên bảng đổi sang temp_name
              -- regex neo đầu dòng (^ORDER BY .*$, multiline) — đã verify dòng đó luôn
                 đứng riêng, không lẫn PARTITION BY/TTL/SETTINGS/COMMENT/INDEX phụ

CREATE (new_ddl)                                        -- lỗi ở đây: bảng gốc chưa hề bị đụng
INSERT INTO db.temp_name SELECT * FROM db.table         -- lỗi ở đây: DROP temp_name rồi trả lỗi,
                                                         -- bảng gốc vẫn nguyên
old_count = SELECT count() FROM db.table
new_count = SELECT count() FROM db.temp_name
nếu old_count != new_count:
    DROP TABLE db.temp_name
    trả lỗi "số dòng lệch khi copy (có ghi đồng thời?), đã huỷ an toàn, bảng gốc không đổi, thử lại"
ngược lại:
    EXCHANGE TABLES db.table AND db.temp_name           -- atomic, đã verify trên Atomic database
    DROP TABLE db.temp_name                             -- giờ giữ dữ liệu CŨ
    nếu DROP này lỗi:
        -- sorting key ĐÃ đổi thành công — đây không phải thất bại của thao tác chính
        trả về cảnh báo (không phải lỗi): "đã đổi xong, còn sót bảng tạm '{temp_name}' giữ dữ
        liệu cũ, tự xoá tay khi tiện"
```

Chữ ký: `pub async fn rebuild_order_by(conn, database, table, columns: &[String]) ->
Result<Option<String>, AppError>` — `Ok(Some(temp_name))` là nhánh cảnh báo cuối cùng ở trên (tráo
xong, dọn lỗi), `Ok(None)` là xong sạch. Lệnh Tauri `clickhouse_rebuild_order_by` trả nguyên
`Option<String>`, frontend hiện banner cảnh báo (không phải lỗi) khi có giá trị.

`PRIMARY KEY (...)` khai riêng (khác `ORDER BY`), nếu có ở bảng gốc, không bị regex đụng tới — nếu
key mới không còn chứa nó làm tiền tố, `CREATE` bảng tạm tự thất bại với lỗi rõ ràng của ClickHouse
(không phải silent-corrupt) vì bảng gốc chưa hề bị đụng ở bước đó. Không cần xử lý gì thêm, chỉ ghi
nhận đây là một lỗi "an toàn" chứ không phải một trường hợp phải chặn trước ở dialog.

**D8 — Rủi ro đã biết, chấp nhận, không giải được ở v1 (ghi rõ trong thông báo/tooltip, không giả vờ
là đã xử lý hết):**

- So `count()` chỉ bắt lệch **số dòng**, không bắt lệch **nội dung** — nếu trong lúc copy có N dòng bị
  xoá và N dòng khác được insert, count khớp nhưng dữ liệu bảng tạm đã khác bảng gốc lúc bắt đầu.
  ClickHouse không có `LOCK TABLE`; không có cách nào loại bỏ hoàn toàn rủi ro này nếu không khoá ghi.
- Không huỷ được giữa chừng (`cancellable: false` sẵn có cho ClickHouse) — bảng lớn nghĩa là spinner
  treo nhiều phút, không có nút Huỷ.
- Nếu app/kết nối rớt giữa chừng, `temp_name` là một bảng thật, hiện trong danh sách bảng của sidebar
  nếu người dùng mở lại — tên đã đủ tự giải thích (`__<old>_rebuild_<timestamp>`) để không gây hoang
  mang, nhưng không có cơ chế dọn tự động nào khác ngoài việc người dùng tự xoá.

**D9 — Không tiền-hạn chế theo engine trong `SkipIndexDialog`** — thêm skip index chạy được trên bất
kỳ engine MergeTree family nào (kể cả Replicated), vì đây là ALTER nhẹ, không rebuild, không có rủi ro
lệch cluster như D11 dưới.

**D10 — `SqlApi.rowCount` (method mới): `notSupported()` cho MySQL/Postgres/SQLite, thật cho
ClickHouse.** Chỉ phục vụ D6 — không phải một tính năng đếm dòng chung cho mọi engine, ba dialect kia
đã có cách khác để biết số dòng (phân trang Data tab).

**D11 — Guard engine cho rebuild: nút Edit trên dòng `sorting_key` tắt (kèm tooltip giải thích) nếu
`SqlTableStructure.engine` không nằm trong 4 engine đã whitelist ở D2 spec DDL chính (`MergeTree`,
`ReplacingMergeTree`, `SummingMergeTree`, `AggregatingMergeTree`).**

Structure tab hoạt động trên *bất kỳ* bảng đã tồn tại, kể cả bảng tạo bởi công cụ khác — có thể là
`ReplicatedMergeTree`. `EXCHANGE TABLES`/`CREATE`/`DROP` không có `ON CLUSTER` sẽ chỉ áp trên một
replica, gây lệch cluster thay vì thất bại rõ ràng. Đọc `system.tables.engine` (đã verify rẻ) và so
với whitelist — cùng tinh thần "không tạo/sửa engine ngoài whitelist" đã có ở D2 của spec DDL chính,
áp lại cho *rebuild trên bảng đã có* thay vì chỉ *tạo mới*.

## Backend — file đổi

```
src-tauri/src/modules/db/drivers/clickhouse.rs
  table_structure: + đọc system.data_skipping_indices (skip_indexes), system.tables.engine (D2)
  + struct SkipIndex { name, expr, index_type, args: Vec<String>, granularity }
  + fn parse_type_full(type_full: &str) -> (String, Vec<String>)   (D2, tách tên/tham số)

src-tauri/src/modules/db/drivers/clickhouse_ddl.rs
  + pub async fn add_skip_index(conn, database, table, spec: &SkipIndexSpec) -> Result<(), AppError>
  + pub async fn drop_skip_index(conn, database, table, name) -> Result<(), AppError>
  + pub async fn modify_skip_index(conn, database, table, old_name, spec) -> Result<(), AppError>
      -- DROP INDEX rồi ADD INDEX, hai câu tách rời (D4)
  + pub async fn rebuild_order_by(conn, database, table, columns: &[String])
        -> Result<Option<String>, AppError>                                            (D7)
  + pub async fn row_count(conn, database, table) -> Result<u64, AppError>             (D10)

src-tauri/src/modules/db/commands/clickhouse.rs
  + clickhouse_add_skip_index, clickhouse_modify_skip_index, clickhouse_drop_skip_index,
    clickhouse_rebuild_order_by, clickhouse_row_count

src-tauri/src/modules/mod.rs
  + năm dòng generate_handler! cho năm lệnh trên
```

## Frontend — file đổi

```
src/modules/db/types.ts
  + interface SqlSkipIndex { name; expr; indexType; args: string[]; granularity: number }
  + interface SqlSkipIndexSpec  (cùng shape)
  + SqlTableStructure: + skipIndexes: SqlSkipIndex[]; + engine: string | null           (D2)

src/modules/db/sql/api.ts
  SqlApi: + addSkipIndex, modifySkipIndex, dropSkipIndex, rebuildOrderBy, rowCount

src/modules/db/mysql/api.ts, postgres/api.ts, sqlite/api.ts
  + năm method mới, đều notSupported()

src/modules/db/clickhouse/api.ts
  + năm method mới: invoke() thật thay notSupported()
  (addIndex/modifyIndex/dropIndex GIỮ NGUYÊN notSupported() — không đổi, xem D1)

src/modules/db/clickhouse/skipIndexTypes.ts   (file mới)
  Whitelist 5 TYPE + số tham số + hint text (D3)

src/modules/db/components/SkipIndexDialog/
  SkipIndexDialog.tsx, SkipIndexDialog.module.css, index.ts     (mới, D3/D4)

src/modules/db/components/OrderByDialog/
  OrderByDialog.tsx, OrderByDialog.module.css, index.ts         (mới, D6)

src/modules/db/components/TableStructure/TableStructure.tsx
  + panel "Skip indexes" (add/edit/drop, chỉ render kind === "clickhouse")             (D5)
  panel "Indexes": nút Add + điều kiện offers.indexKinds.length === 0 vào disabled;
                    dòng sorting_key: Edit mở OrderByDialog (kind==="clickhouse" &&
                    index.indexType === "sorting_key"), Drop thêm điều kiện tắt,
                    Edit thêm điều kiện tắt theo D11 (engine không trong whitelist)     (D5, D11)

src/modules/db/i18n/en.ts, vi.ts
  + chuỗi cho hai dialog mới, panel mới, banner cảnh báo "đã đổi xong, còn sót bảng tạm ..." (D7),
    lỗi lệch số dòng (D7), tooltip guard engine (D11), hint GRANULARITY (D3)
```

## Kiểm thử

**Rust, thuần** (`cargo test`, chạy CI):

- `parse_type_full`: `"minmax"` → `("minmax", [])`; `"set(100)"` → `("set", ["100"])`;
  `"ngrambf_v1(3, 256, 2, 0)"` → tách đúng 4 phần tử, giữ khoảng trắng đã trim.
- `rebuild_order_by`'s phần dựng câu (tách khỏi phần gửi HTTP): regex thay đúng dòng `ORDER BY` giữ
  nguyên `PARTITION BY`/`TTL`/`SETTINGS`/`COMMENT`/index phụ trong khối cột; tên bảng tạm có đúng
  hậu tố `__<old>_rebuild_<millis>`; trường hợp `ORDER BY` rỗng (`tuple()`) và có sẵn `PRIMARY KEY`
  riêng (không bị đụng).
- `modify_skip_index`: đúng thứ tự hai câu lệnh, `DROP INDEX` trước `ADD INDEX`.
- Chuỗi câu lệnh `add_skip_index` cho từng TYPE: đúng cú pháp `INDEX name expr TYPE
  type(args) GRANULARITY n`, `GRANULARITY` luôn có mặt kể cả khi người dùng để mặc định.

**Vitest, thuần:**

- `skipIndexTypes.ts`: số tham số đúng theo bảng đã verify (đặc biệt `tokenbf_v1` 3 không phải 4).
- `OrderByDialog`'s danh sách cột: add/remove giữ đúng thứ tự, không cho submit khi rỗng.
- Nút Confirm của `OrderByDialog`: khoá cho tới khi ô nhập khớp *chính xác* tên bảng (phân biệt
  hoa/thường).

**Bằng tay, ghi vào báo cáo cuối** (server thật `clickhouse-test-server`, throwaway, xoá sau khi xong):

- Thêm skip index từng TYPE trong whitelist, kiểm `SHOW CREATE TABLE` ra đúng cú pháp.
- Sửa một skip index (đổi TYPE) — xác nhận đúng hai câu `DROP`/`ADD` chạy (đối chiếu
  `system.query_log` nếu cần), không phải một câu `MODIFY` (vốn không tồn tại).
- Xoá skip index.
- Rebuild sorting key trên một bảng có dữ liệu thật — xác nhận `SHOW CREATE TABLE` sau đó ra đúng
  `ORDER BY` mới, dữ liệu còn nguyên (so `count()` và vài dòng mẫu trước/sau), không còn bảng tạm nào
  sót lại (`SHOW TABLES` không còn `__<old>_rebuild_...`).
- Rebuild trên bảng có `PRIMARY KEY` khai riêng khác `ORDER BY`, chọn key mới không chứa PK cũ làm
  tiền tố — xác nhận lỗi CREATE rõ ràng, bảng gốc không đổi, không có bảng tạm sót lại.
- Nút Edit dòng `sorting_key` bị tắt trên một bảng test dựng với tên engine không thuộc whitelist
  (xác nhận qua thay `system.tables.engine` — không cần dựng `ReplicatedMergeTree` thật trên server
  một node; ghi rõ trong báo cáo là **chưa test tay trên bảng Replicated thật**, chỉ test đường
  guard bằng dữ liệu giả).
- Nút Add của panel "Indexes" bị tắt cho ClickHouse (xác nhận sửa gap có sẵn ở D5).

## Rủi ro

- **So `count()` không bắt được lệch nội dung nếu có ghi đồng thời trong lúc copy — chấp nhận, không
  giải được (D8).** Rủi ro lớn nhất còn lại của spec này.
- **Không huỷ giữa chừng, không progress bar (D8/Phi mục tiêu).** Bảng lớn = trải nghiệm chờ dài,
  không có lối thoát ngoài đợi xong hoặc đóng cả kết nối.
- **Bảng tạm mồ côi nếu app crash giữa chừng (D8).** Giảm nhẹ bằng tên tự giải thích, không dọn tự
  động được.
- **Chưa test tay trên bảng `ReplicatedMergeTree` thật — chỉ test đường guard bằng dữ liệu giả (D11,
  Kiểm thử).** Nếu guard có lỗi logic không bắt được bằng test giả, rebuild trên bảng Replicated có
  thể chạy và gây lệch cluster — rủi ro thấp (guard đơn giản, một phép so chuỗi) nhưng chưa loại trừ
  hoàn toàn bằng test tay.

## Những gì để lại

- **Chọn `ORDER BY` ngay lúc `CREATE TABLE`** — xem Phi mục tiêu; cải tiến UX thật, nhưng đụng lại
  quyết định D2 đã merge của spec DDL chính, không mở ở đây.
- **Giới hạn TYPE skip index theo kiểu cột, kiểm dung lượng đĩa trước rebuild, progress bar/huỷ giữa
  chừng, `ON CLUSTER`/bảng Replicated** — xem Phi mục tiêu.
