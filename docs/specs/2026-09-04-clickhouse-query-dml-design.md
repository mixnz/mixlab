---
status: implemented
date: 2026-09-04
---

# ClickHouse: mở Query tab cho DML

Ngày: 2026-09-04

## Mục tiêu

Cho phép gõ tay bốn verb trong Query tab trên kết nối ClickHouse: `INSERT`, `ALTER TABLE ... UPDATE
... WHERE ...` (cách ClickHouse viết `UPDATE`), `DELETE FROM ... WHERE ...` (lightweight delete),
`TRUNCATE TABLE`. Đây là mảnh cuối cùng còn thiếu của "ghi được" trên ClickHouse — Data tab (grid)
và Structure tab (DDL) đã ghi được từ hai phase trước, chỉ riêng Query tab vẫn `readOnly` toàn bộ.

Nguồn: mục "Những gì để lại" của
[`2026-09-04-clickhouse-row-writes-design.md`](2026-09-04-clickhouse-row-writes-design.md) —

> **Mở Query tab cho DML** (`INSERT`/`UPDATE`/`DELETE`/`TRUNCATE` gõ tay) — nếu làm, đây là lúc
> `guard.ts::writingStatements` mới thật sự cần tổng quát hoá sang tập verb thay vì boolean, đúng
> như bản phân tích ban đầu đã cân nhắc rồi bỏ ở D6. Không làm trước khi có nhu cầu thật.

## Phi mục tiêu

- **DDL gõ tay** (`CREATE`/`DROP`/`ALTER` ngoài hình dạng `UPDATE`) qua Query tab — vẫn đi qua
  Structure tab, nơi đã có dialog riêng biết chính xác đối tượng và có xác nhận phù hợp. `writable`
  vẫn `false`; Query tab chỉ mở thêm bốn verb DML, không mở toàn bộ.
- **`ALTER TABLE t DELETE WHERE ...`** (cú pháp mutation delete cũ hơn) — chỉ hỗ trợ `DELETE FROM
  ... WHERE ...` (lightweight), đúng cách ClickHouse khuyến nghị dùng cho xoá có điều kiện. Gõ
  `ALTER TABLE t DELETE WHERE ...` vẫn bị chặn như DDL khác — quyết định giữ phạm vi hẹp, xem D2.
- **`ALTER TABLE t UPDATE ...` có nhiều mệnh đề gộp dấu phẩy** (vd trộn `DROP COLUMN` và `UPDATE`
  trong cùng một câu) — không nhận diện, vẫn bị chặn như DDL. Chỉ nhận hình dạng đơn giản nhất: một
  `ALTER TABLE <tên> UPDATE ... [WHERE ...]` không kèm mệnh đề nào khác. Xem D3.
- **Báo số dòng chính xác bị ảnh hưởng bởi UPDATE/DELETE** — ClickHouse không cho biết trước con số
  đó lúc mutation được enqueue (xác minh trên server thật: `written_rows` trong
  `X-ClickHouse-Summary` luôn là `0` cho `ALTER TABLE ... UPDATE`/`DELETE FROM ... WHERE`, khác hẳn
  `INSERT`). `kind: "ok"` cho hai verb này, không bịa số qua một `SELECT count()` phụ trội — cùng
  tinh thần D8 của spec row-writes ("không cố sửa ở tầng đọc/ghi").
- **Pre-check `matched = N` trước khi UPDATE/DELETE** — đây là lưới an toàn D3 của spec row-writes
  cho *grid* (nơi `key` luôn có cấu trúc, xác định đúng 1 dòng). Query tab nhận `WHERE` tự do người
  dùng gõ, có thể chủ đích khớp nhiều dòng — không áp safety net đó ở đây, đúng cách MySQL/Postgres/
  SQLite đã hoạt động (không pre-check, chỉ có dialog "gõ tên bảng để xác nhận" khi thiếu `WHERE`).
- **Cancel giữa chừng cho mutation gõ tay** — `cancellable` vẫn `false`, giống D4 của spec
  row-writes cho grid.
- **Progress bar.**
- **`WITH` dẫn vào DML** — điểm mù MySQL 8 mà `guard.ts::actualVerb` đã xử lý không áp dụng cho
  ClickHouse: ClickHouse không có cú pháp CTE dẫn vào `INSERT`/`ALTER TABLE ... UPDATE`/`DELETE`,
  CTE ở đây chỉ đứng trước `SELECT`. Không cần thêm gì.
- **`EXPLAIN` chạy DML** — xác minh trên server thật: không biến thể `EXPLAIN` nào của ClickHouse
  (`AST`, `SYNTAX`, `PLAN`, `PIPELINE`, `ESTIMATE`) thực thi statement được đưa vào (khác hẳn
  `EXPLAIN ANALYZE` của MySQL/Postgres mà `guard.ts::explainRuns` phải xử lý riêng) —
  `EXPLAIN AST INSERT INTO ... VALUES (...)` không chèn dòng nào, đã kiểm tra bằng `SELECT count()`
  ngay sau. `judged()`/`explainRuns()` không cần sửa gì cho ClickHouse.

## Hiện trạng liên quan

- `clickhouseDialect.writable` là `false` — gác DDL/dump-restore/toàn bộ Query tab (xem doc comment
  của `SqlDialect.writable` tại [`dialect.ts:126-140`](../../apps/desktop/src/modules/db/sql/dialect.ts)).
- `clickhouseDialect.rowsWritable` **đã là `true`** — và doc comment của field này (viết từ phase
  row-writes, [`dialect.ts:154-161`](../../apps/desktop/src/modules/db/sql/dialect.ts)) đã nói đúng ý của
  phase này: *"the Query tab may send INSERT/UPDATE/DELETE/TRUNCATE — independent of writable...
  the Query tab is not wired to this flag yet"*. Phase này nối dây đúng như đã dự tính, không cần
  field mới trên `SqlDialect`.
- `clickhouse_script::run()` gửi mỗi statement qua `query_in_database`, luôn nối `\nFORMAT JSON`.
  **Xác minh trên server thật (26.8, `<old>_agent_test`, dọn sạch sau khi test) rằng điều này vỡ với
  DML:**
  - `INSERT INTO t (...) VALUES (...)\nFORMAT JSON` → `400`, `Code: 27. ... Cannot parse input:
    expected '(' before: 'FORMAT JSON'` — `FORMAT` sau `VALUES` bị hiểu là định dạng *dữ liệu đầu
    vào* của `INSERT`, không phải định dạng kết quả trả về.
  - `DELETE FROM t WHERE ...\nFORMAT JSON` → `400`, `Code: 62. Syntax error ... FORMAT JSON`. Ngữ
    pháp `DELETE FROM` không nhận một `FORMAT` ở cuối như `SELECT` nhận.
  - `ALTER TABLE t UPDATE ... WHERE ...\nFORMAT JSON` và `TRUNCATE TABLE t\nFORMAT JSON` — cả hai
    **đều thành công** (`200`), nhưng đây là chỗ hai statement này tình cờ dung nạp một `FORMAT`
    thừa chứ không phải điều nên dựa vào — vẫn đi qua đường gửi không-`FORMAT` cho nhất quán với
    `INSERT`/`DELETE`.
- `clickhouse::run_mutation_and_wait(conn, database, table, command_sql)` đã tồn tại nguyên vẹn cho
  grid (D4 của spec row-writes): gửi qua `execute_check`, poll `system.mutations` tối đa 30s bằng
  `mutation_id` không nằm trong baseline. Hàm này không quan tâm `command_sql` là `UPDATE` hay
  `DELETE` — chỉ cần đúng `(database, table)`. **Xác minh trên server thật:** cả `ALTER TABLE ...
  UPDATE ... WHERE` lẫn `DELETE FROM ... WHERE` đều tạo dòng trong `system.mutations` (delete tạo
  mutation dạng `UPDATE _row_exists = 0 WHERE ...` — cùng cơ chế nội bộ, cùng cột theo dõi). Tái
  dùng được nguyên hàm này cho cả hai verb, không cần viết lại.
- `X-ClickHouse-Summary` (header HTTP) trả `written_rows` — xác minh chính xác cho `INSERT` đồng bộ
  (`written_rows: 2` khớp đúng số dòng vừa chèn), luôn `0` cho mutation lúc submit (như trên).
- `guard.ts::unguardedWrites` hiện **bỏ qua hoàn toàn** `ALTER TABLE t UPDATE ... WHERE ...`: nhánh
  `verb === "ALTER"` chỉ gọi `dropTarget()` khi `clauses` có từ `DROP`
  ([`guard.ts:216-220`](../../apps/desktop/src/modules/db/sql/guard.ts)); không có `DROP` → `continue`, không
  báo gì. Nghĩa là gõ `ALTER TABLE users UPDATE status = 'x'` thiếu `WHERE` sẽ **không** hỏi xác
  nhận nếu không sửa — lỗ hổng thật, phải vá trong phase này (D3). `DELETE FROM ... WHERE ...` đã
  đúng ngay từ đầu qua nhánh chung (kiểm `WHERE`/`LIMIT` trong `clauses`), không cần sửa.

## Quyết định đã chốt

**D1 — Dispatch theo verb ở backend, không đổi cấu trúc `run()`.**

`clickhouse_script.rs` thêm một bước phân loại trước khi gửi mỗi statement, dựa trên `verb` đã có
sẵn từ `split_statements` cộng một lượt quét thêm cho hình dạng `ALTER`:

| Verb | Cách gửi | `kind` trả về |
|---|---|---|
| `INSERT` | `execute_check`-style (không `FORMAT`), đọc `written_rows` từ `X-ClickHouse-Summary` | `"affected"`, `rowsAffected = written_rows` |
| `TRUNCATE` | `execute_check`-style (không `FORMAT`), không poll | `"ok"` |
| `DELETE` (tức `DELETE FROM ... WHERE`) | trích `(database, table)` sau `FROM`, gọi `run_mutation_and_wait` | `"ok"` |
| `ALTER` khớp hình dạng D3 | trích `(database, table)` sau `TABLE`, gọi `run_mutation_and_wait` | `"ok"` |
| Mọi verb khác | giữ nguyên `query_in_database` (có `FORMAT JSON`) | như hiện tại |

`run_mutation_and_wait` dùng lại y nguyên, không sửa chữ ký — nó vốn đã nhận `command_sql` là văn
bản tuỳ ý.

**D2 — Chỉ hỗ trợ `DELETE FROM ... WHERE` (lightweight), không hỗ trợ `ALTER TABLE t DELETE WHERE`
(mutation delete cũ).**

Hai cú pháp cùng đạt một việc — `DELETE FROM` là cách ClickHouse khuyến nghị dùng, và đã khớp sẵn
với nhánh chung có sẵn của `guard.ts` không cần sửa gì. Thêm cả `ALTER TABLE ... DELETE WHERE` là
thêm một hình dạng thứ hai cho cùng một khái niệm — đi ngược tinh thần "không làm trước khi có nhu
cầu thật" của chính spec này. Gõ `ALTER TABLE t DELETE WHERE ...` vẫn rơi vào diện DDL bị chặn.

**D3 — Nhận diện `ALTER TABLE <tên> UPDATE ... [WHERE ...]`: chỉ hình dạng đơn giản nhất, một mệnh
đề, không gì khác.**

Một hàm dùng chung cho cả guard.ts (dialog xác nhận) lẫn backend (dispatch + trích tên bảng):
statement có `verb === "ALTER"`, từ tiếp theo (bỏ qua rỗng) là `TABLE`, sau đó một tên bảng, sau đó
đúng từ `UPDATE`, và **không có dấu phẩy ở top level** trước `WHERE`/hết câu (dấu phẩy ở top level
nghĩa là nhiều `AlterCommand` gộp chung — ví dụ trộn `DROP COLUMN` và `UPDATE` trong cùng một
`ALTER TABLE`, ClickHouse cho phép cú pháp này). Có dấu phẩy → không nhận diện, statement vẫn rơi
vào diện DDL bị chặn như trước — mặc định an toàn, không đoán.

Việc này cần một tokenizer nhỏ ở phía Rust (backend không có sẵn bộ `topLevelWords` như guard.ts)
— quét ký tự tôn trọng quote backtick/double-quote/chuỗi đơn giống hệt quy tắc đã có trong
`split_statements`, không cần theo dõi độ sâu ngoặc (tên bảng trong `ALTER TABLE`/`FROM` không bao
giờ là subquery).

**D4 — Trích `(database, table)` từ văn bản câu lệnh; thiếu database thì dùng ngữ cảnh đang mở.**

`ALTER TABLE mydb.mytable UPDATE ...` → `database = "mydb"`. `ALTER TABLE mytable UPDATE ...`
(không qualify) → `database` lấy từ tham số `database: Option<&str>` đã có sẵn của `run()` (ngữ
cảnh database đang chọn ở sidebar) — đúng cách `query_in_database` đã dùng `?database=` cho mọi
câu đọc không qualify tên bảng.

**D5 — `writingStatements()` (guard.ts) học thêm một ngoại lệ hẹp: bốn verb DML được phép ngay cả
khi `dialect.writable = false`, nếu lý do là dialect chứ không phải connection bị khoá tay.**

Không đổi chữ ký `writingStatements(statements, dialect)` thành một tham số options mới — thay vào
đó, `SqlWorkspace`/`QueryEditor` chuyển từ truyền một `readOnly: boolean` đã gộp OR sẵn, sang thêm
**một prop mới** song song ba prop đã có (`readOnly`/`schemaReadOnly`/`dataReadOnly` ở
[`DbTab.tsx:767-772`](../../apps/desktop/src/modules/db/DbTab.tsx)):

```ts
// DbTab.tsx — thêm dòng thứ tư, cùng khuôn với ba dòng đã có
dmlEvenIfReadOnly={!(activeSavedConnection?.readOnly ?? false) && engine.dialect.rowsWritable}
```

`true` chỉ khi: connection **không** bị đánh dấu read-only tay, **và** dialect cho phép rows-DML.
`QueryEditor`'s gate (hiện ở [`QueryEditor.tsx:428`](../../apps/desktop/src/modules/db/components/QueryEditor/QueryEditor.tsx))
đổi thành:

```ts
if (readOnly) {
  const writes = writingStatements(statements, dialect)
    .filter((w) => !(dmlEvenIfReadOnly && isRowsDml(w.statement, dialect)));
  if (writes.length > 0) { /* như cũ */ }
}
```

`isRowsDml` là verb `INSERT`/`TRUNCATE`/`DELETE`, hoặc `ALTER` khớp hình dạng D3 — hàm dùng chung
với D3. Với MySQL/Postgres/SQLite, `dmlEvenIfReadOnly` chỉ `true` khi `readOnly` (dialect phần)
vốn đã `false` sẵn (`writable` của cả ba luôn `true`) — nhánh mới không bao giờ kích hoạt, không
đổi hành vi ba engine đó. Connection bị khoá tay luôn thắng tuyệt đối: `readOnly` (đã gộp OR) vẫn
`true`, còn `dmlEvenIfReadOnly` tính từ *connection không khoá* nên tự động `false` — ngoại lệ
không bao giờ áp dụng, đúng như đã chốt.

Lệnh gọi `writingStatements()` thứ hai trong file (dòng ~522, phát hiện "script này có ghi không"
để invalidate schema cache sau khi chạy xong) **không đổi** — nó không nằm trong nhánh `if
(readOnly)`, và việc phát hiện ALTER/DELETE/INSERT/TRUNCATE là write đã đúng sẵn không cần hình
dạng D3 (chỉ cần verb không nằm trong `READ_VERBS`, đã đúng từ trước).

**D6 — Dialog "gõ tên bảng để xác nhận" (`unguardedWrites`) nhận thêm nhánh cho hình dạng D3.**

Statement khớp D3 (`ALTER TABLE <tên> UPDATE ... [WHERE ...]`) được coi như một write kiểu
`"rows"` — cùng loại `UPDATE`/`DELETE` thường, kiểm `WHERE`/`LIMIT` trong mệnh đề sau `UPDATE`
giống hệt nhánh chung hiện có. Thiếu `WHERE` → vào diện "ghi đè toàn bảng", đúng UX ba engine kia
đã có. Nhánh `ALTER`-với-`DROP` hiện có không đổi — hai nhánh (D3-shape vs DROP-shape) tách biệt
theo có/không từ `UPDATE`/`DROP` ngay sau tên bảng.

**D7 — Badge "Chỉ đọc" trên Query tab đổi thành "Chỉ khoá DDL" khi lý do là D5's ngoại lệ đang áp
dụng.**

Badge hiện tại ([`QueryEditor.tsx:638-641`](../../apps/desktop/src/modules/db/components/QueryEditor/QueryEditor.tsx))
hiện ra bất cứ khi nào `readOnly === true`, dùng chung `query.readOnly`/`common.readOnlyConnection`
cho mọi lý do. Sau phase này, với ClickHouse (`writable=false`, `rowsWritable=true`, connection
không khoá tay), giữ nguyên chữ "Chỉ đọc" sẽ sai — INSERT/UPDATE/DELETE/TRUNCATE chạy thật. Thêm:

```tsx
{readOnly && (
  <span className={styles.readOnly} title={t(dmlEvenIfReadOnly ? "query.ddlOnlyReadOnlyHint" : "common.readOnlyConnection")}>
    {t(dmlEvenIfReadOnly ? "query.ddlOnlyReadOnly" : "query.readOnly")}
  </span>
)}
```

Khoá i18n mới, cả `en.ts`/`vi.ts`, cạnh `query.readOnly` hiện có:
- `query.ddlOnlyReadOnly`: `"DDL locked"` / `"Chỉ khoá DDL"`.
- `query.ddlOnlyReadOnlyHint`: `"INSERT/UPDATE/DELETE/TRUNCATE work here. Table and database
  changes still go through the Structure tab."` / `"INSERT/UPDATE/DELETE/TRUNCATE chạy được ở
  đây. Đổi bảng và database vẫn phải qua Structure tab."`

Thông điệu chặn (`query.readOnlyBlocked`, hiện "this connection is marked read-only...") cũng sai
theo cùng lý do khi ai đó gõ tay `CREATE`/`DROP`/`ALTER` khác hình dạng D3 trên ClickHouse — không
phải vì connection bị khoá. Thêm khoá mới `query.ddlBlocked`: `"Nothing was sent: ClickHouse's
Query tab only takes INSERT/UPDATE/DELETE/TRUNCATE by hand — other changes go through the
Structure tab."` / `"Không có gì được gửi: Query tab của ClickHouse chỉ nhận INSERT/UPDATE/DELETE/
TRUNCATE gõ tay — thay đổi khác phải qua Structure tab."`. Dùng khi `dmlEvenIfReadOnly === true`
nhưng statement bị chặn vẫn không phải rows-DML (tức là DDL thật). MySQL/Postgres/SQLite/kết nối
bị khoá tay giữ nguyên `query.readOnlyBlocked` như cũ.

**D8 — Không thêm field mới trên `SqlDialect`.**

`rowsWritable` (đã có, đã `true` cho ClickHouse) là đủ — đúng như doc comment của chính field này
đã dự tính từ phase row-writes. Không cần một `dmlWritable` hay `queryDmlVerbs` riêng.

## Backend — file đổi

- `src-tauri/src/modules/db/drivers/clickhouse_script.rs` — bước phân loại D1, tokenizer trích
  `(database, table)` cho D3/D4, hàm đọc `X-ClickHouse-Summary` cho `written_rows`.
- `src-tauri/src/modules/db/drivers/clickhouse.rs` — không đổi; `run_mutation_and_wait` dùng lại
  nguyên trạng qua `pub(super)` đã có (kiểm lại đúng mức lộ ra đủ cho `clickhouse_script.rs` gọi
  tới, hai file đã cùng module nên khả năng cần chỉnh chỉ là visibility, không phải logic).

## Frontend — file đổi

- `src/modules/db/sql/guard.ts` — hàm nhận diện hình dạng D3 (dùng chung D3/D6), nhánh mới trong
  `unguardedWrites()` (D6).
- `src/modules/db/components/QueryEditor/QueryEditor.tsx` — prop mới `dmlEvenIfReadOnly`, gate ở
  D5, badge + thông điệp chặn ở D7.
- `src/modules/db/sql/SqlWorkspace.tsx` — prop mới `dmlEvenIfReadOnly`, chuyển tiếp xuống
  `QueryEditor` (cùng cách `dataReadOnly` đã được chuyển xuống `SqlTable`).
- `src/modules/db/DbTab.tsx` — dòng tính `dmlEvenIfReadOnly` (D5).
- `src/modules/db/i18n/en.ts`, `vi.ts` — bốn khoá mới của D7.

## Kiểm thử

- **Song song hai bộ test** cho hàm nhận diện D3 — một ở `guard.test.ts` (nếu có, hoặc file test
  của `unguardedWrites`/`writingStatements`), một ở `clickhouse_script.rs`'s `#[cfg(test)] mod
  tests` — cùng bộ case, đúng quy ước module doc của `clickhouse_script.rs` đã nói ("a change to
  either splitter belongs in the same commit as the other, and in both sets of tests").
  - `ALTER TABLE t UPDATE x = 1 WHERE id = 2` → nhận diện, trích được `t`.
  - `ALTER TABLE db.t UPDATE x = 1` (không `WHERE`) → nhận diện, `unguardedWrites` báo thiếu WHERE.
  - `ALTER TABLE t DROP COLUMN x, UPDATE y = 1 WHERE z = 2` (nhiều mệnh đề) → **không** nhận diện.
  - `ALTER TABLE t DROP COLUMN x` → không nhận diện là D3-shape (đi nhánh DROP hiện có, không đổi).
  - `ALTER TABLE t DELETE WHERE id = 1` → không nhận diện (D2 — ngoài phạm vi).
- Test tay trên server thật (`<old>_agent_test`) trước khi coi plan xong, tương tự cách D4 của spec
  row-writes từng bắt được lỗi so khớp `command` sai:
  - `INSERT` qua Query tab → `kind: "affected"`, số đúng bằng số dòng vừa gõ.
  - `ALTER TABLE ... UPDATE ... WHERE` khớp nhiều dòng → chạy xong, dữ liệu đổi đúng, không timeout
    giả trên bảng nhỏ.
  - `DELETE FROM ... WHERE` không khớp dòng nào → vẫn trả `"ok"` (mutation "thành công" dù không
    đổi gì — đúng ngữ nghĩa ClickHouse, không phải lỗi).
  - `INSERT INTO t SELECT ... FROM other_t` (insert-select, không phải `VALUES`) — xác nhận đường
    gửi không-`FORMAT` vẫn đúng cho hình dạng này, và `written_rows` vẫn phản ánh đúng.
  - Một script nhiều câu: `INSERT ...; ALTER TABLE t UPDATE ... WHERE ...; SELECT * FROM t` — câu
    `SELECT` cuối phải thấy dữ liệu đã đổi (xác nhận `run()` đợi mutation xong trước khi chạy câu
    kế, không có race).
  - Connection bị đánh dấu read-only tay + ClickHouse: gõ `INSERT` vẫn bị chặn, badge vẫn "Chỉ đọc"
    (không phải "Chỉ khoá DDL") — xác nhận D5's điều kiện kép hoạt động đúng chiều ưu tiên.

## Rủi ro

- **Hai bản nhận diện hình dạng D3 (TS và Rust) có thể lệch nhau theo thời gian** — cùng rủi ro đã
  chấp nhận sẵn giữa `statements.ts`/`guard.ts` và `split_statements` của Rust; giảm nhẹ bằng bộ
  test song song ở trên, không giải được triệt để (không có cách chia sẻ code giữa hai ngôn ngữ ở
  kiến trúc hiện tại).
- **`ALTER TABLE` còn hình dạng khác chưa tính tới** (ví dụ cú pháp tương lai của ClickHouse) —
  D3's yêu cầu "không dấu phẩy ở top level" là mặc định an toàn (không nhận diện nhầm), nhưng cũng
  có nghĩa một số câu UPDATE hợp lệ nhưng viết theo cách lạ có thể bị từ chối oan (rơi về diện DDL
  bị chặn) — chấp nhận, sửa sau nếu có người dùng thật gặp phải.
- **Không pre-check cho UPDATE/DELETE gõ tay** (D — Phi mục tiêu) — người dùng chịu trách nhiệm
  hoàn toàn cho `WHERE` họ gõ, giống MySQL/Postgres/SQLite. Dialog D6 là lưới an toàn duy nhất khi
  quên `WHERE`, không bắt được `WHERE` sai nhưng khớp nhiều dòng có chủ đích nhầm.
- **Giới hạn đã biết của `run_mutation_and_wait`** (mutation khác chen ngang cùng lúc, poll tới hết
  30s mới báo lỗi) — kế thừa nguyên trạng từ D4 của spec row-writes, không phải rủi ro mới của
  phase này.

## Những gì để lại

- **Dump/restore** — phase riêng,
  [`2026-09-04-clickhouse-dump-restore-design.md`](2026-09-04-clickhouse-dump-restore-design.md).
- **Cancel cho mutation đang chờ** — `KILL MUTATION`/`KILL QUERY` cần tracking `query_id`/
  `mutation_id` — D4 của spec row-writes đã nói, vẫn còn nguyên, giờ áp dụng cho cả mutation gõ tay
  qua Query tab.
- **`ALTER TABLE t DELETE WHERE ...`** — xem D2, có thể mở nếu có nhu cầu thật.
- **`ALTER TABLE` nhiều mệnh đề gộp** — xem D3/Rủi ro, có thể mở rộng tokenizer nếu cần.
- **Nhãn "Chỉ đọc" của `clickhouseReadOnly`/`dump`/`restore`** — chữ hiện tại ("the standalone client only reads
  from ClickHouse for now") đã sai từ trước phase này (grid/DDL đã ghi được) và càng sai hơn sau
  phase này; nằm ngoài phạm vi (khoá đó chỉ dùng cho `dump`/`restore`/index, chưa động tới trong
  spec này) — để phase dump/restore sửa cùng lúc.
