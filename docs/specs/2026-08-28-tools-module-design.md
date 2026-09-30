---
status: implemented
date: 2026-08-28
---

# Module Tools

Ngày: 2026-08-28

## Mục tiêu

The standalone client là một shell cộng các module. Hôm nay có ba: `db`, `rest` và `terminal`. Spec này mô tả module
thứ tư — `tools` — một tập tiện ích nhỏ mà dev chạm tới **trong lúc đang làm việc với DB, API hoặc
máy chủ**, sống cạnh ba module kia mà không bên nào biết khái niệm của bên nào.

Tiêu chí nhận một tool vào module, áp dụng cho mọi đề xuất về sau:

> Chỉ nhận tool nằm trên đường đi của một dev đang làm việc với DB, API hoặc server.

JWT decoder qua được (token của tab REST). Color picker và QR code thì không, dù "cũng hữu ích cho
dev". Tiêu chí này tồn tại vì cái tên "Tools" mời gọi nhét mọi thứ vào, và một ngăn kéo tạp hoá sẽ
làm mờ định vị của app.

Sau khi làm xong giai đoạn 1 và 2:

- Mở tab Tools từ menu `[+]`, chọn một tool ở sidebar trái, dùng nó ở pane phải.
- Tab nhớ tool đang mở giữa các lần khởi động app.
- Sáu tool chạy được: đổi thời gian, mã hoá & băm, đọc JWT, sinh ID hàng loạt, đổi kiểu chữ, và
  dịch SQL sang truy vấn MongoDB.
- Không file nào ngoài `src/modules/tools/` biết khái niệm nào của module, trừ đúng hai dòng mà
  [adding-a-module](../standards/desktop/adding-a-module.md) cho phép.

## Phi mục tiêu

Ghi ra để không bị kéo vào:

- **Không chạy gì cả.** Mọi tool chỉ in ra để copy. Kill port in lệnh chứ không kill; SQL→Mongo in
  truy vấn chứ không thực thi. Đây là ranh giới an toàn của cả module, không phải sự lười.
- **Không bắc cầu sang module khác** ở các giai đoạn này. Nút "tạo saved connection từ chuỗi này"
  hay "chạy truy vấn này ở tab Mongo" đòi một đường đi qua shell mà `ModuleDefinition` chưa có — nó
  là một thiết kế riêng, sau khi module đã đứng vững.
- **Không lưu nội dung người dùng gõ.** Ô vào/ô ra là tạm, mất khi đóng tab. Tab chỉ nhớ *tool nào*
  đang mở. Ngoại lệ duy nhất là cheatsheet ở giai đoạn 4, và nó lưu snippet chứ không lưu input.
- **Không đồng bộ, không tài khoản, không mạng.** Trừ lần quét port ở giai đoạn 4, mọi tool là tính
  toán thuần trong tiến trình frontend.
- **Không verify chữ ký JWT.** Cần secret, và một cái "hợp lệ" sai thì nguy hiểm hơn không nói gì.
- **Không thêm jsdom hay test component.** Repo cố ý chỉ test logic thuần.
- **Không minify JS/CSS.** Xem mục 5.

## Hiện trạng

Những gì đã có sẵn và spec này dựa vào:

| Chỗ | Dùng để làm gì |
| --- | --- |
| `src/shell/module.ts` | `ModuleDefinition` — id, Icon, Tab, `restored`/`onStateChange` |
| `src/shell/registry.ts` | Một dòng nữa trong `MODULES` |
| `src/i18n/dicts.ts` | Một cặp import nữa, và nhóm `error` gộp tay |
| `src/modules/rest/tabState.ts` | Pattern `parseXTabState` — chỗ việc kiểm tra sống |
| `src/modules/rest/RestTab.tsx` | Pattern `<aside>` + pane, và cách một module lazy phần nặng |
| `src/components/` | `Button`, `Input`, `Textarea`, `Select`, `ErrorBanner`, `JsonView` |
| `src/core/clipboard.ts` | Copy sang clipboard, đã dùng ở cả ba module |
| `sql-formatter` | Đã là dependency — giai đoạn 3 dùng lại, không thêm gì |

Ba điều kiểm được trong code, quyết định thiết kế bên dưới:

1. **`ItemList` nhận `items: string[]` phẳng**, không có khái niệm nhóm, và nó gánh cả tìm kiếm,
   ghim, menu chuột phải — thứ mà một danh sách 14 mục cố định không cần. Module dựng danh sách
   riêng của mình thay vì nống `ItemList` ra cho đúng một người dùng.
2. **Chưa có icon nào hợp làm biểu tượng module.** `src/icons/icons.tsx` có 35 icon, không cái nào
   là cờ-lê hay hộp đồ nghề. Thêm `ToolsIcon`.
3. **Chỉ một tool trong cả 14 cần backend.** Quét port ở giai đoạn 4. 13 tool còn lại không gọi
   `invoke` lần nào, nên module này rẻ hơn hẳn ba module kia.

## 1. Bộ khung

```
src/modules/tools/
  index.ts            ModuleDefinition — một dòng trong shell/registry.ts
  ToolsTab.tsx        <aside> danh sách tool  +  pane của tool đang chọn
  tool.ts             ToolDefinition — hợp đồng giữa ToolsTab và mọi tool
  registry.ts         TOOLS: ToolDefinition[]  ← thêm tool = một dòng ở đây
  tabState.ts         parseToolsTabState
  components/ToolList/  danh sách có nhóm, của riêng module này
  tools/<tool-id>/    mỗi tool: Panel.tsx, Panel.module.css, logic .ts, .test.ts
  i18n/{en,vi}.ts     một cặp dòng trong i18n/dicts.ts
  tools.css
```

Điểm thiết kế quan trọng nhất của module là `registry.ts`: **lặp lại kiến trúc shell/module một cấp
xuống**. `ToolsTab` không biết tool nào tồn tại, y như shell không biết module nào tồn tại.

```ts
export interface ToolDefinition {
  id: string;
  labelKey: TranslationKey;
  group: ToolGroup;
  Panel: ComponentType;
}
```

`Panel` không nhận prop nào. Một tool không cần biết nó đang ở tab nào, tab có active không, hay ai
đang mở nó — nó là một ô vào và một ô ra. Hợp đồng nhỏ đến mức này là thứ giữ cho việc thêm tool
thứ 15 vẫn là một file cộng một dòng.

`ToolGroup` là năm nhóm cố định: `data` · `encode` · `time` · `infra` · `text`. Tool SQL→Mongo thuộc
`data`, và nó là mục đầu của nhóm đó.

**Lazy.** `index.ts` lazy `ToolsTab` như `terminal` làm — cùng lý do, cùng một dòng. Bên trong,
`Panel` để eager: mỗi cái là vài trăm dòng React. Thứ nặng là *thư viện*, và nó được `import()`
động trong hàm logic ở lần dùng đầu, không phải ở lần render đầu. Xem mục 4.

**Layout.** `<aside>` rộng cố định cộng pane, không `Splitter`. REST cần chỉnh tay vì tên request là
do người dùng đặt và có thể dài; ở đây danh sách là 14 nhãn ta tự viết. Thêm được lúc nào có ai thấy
chật.

**Badge.** Không có. `ModuleTabProps` nói rõ: không báo badge nào thì shell tự vẽ `Icon` của module —
và một tab Tools không có dấu hiệu nào đáng nói ngoài chính nó.

**Tiêu đề tab.** `onTitleChange` với tên tool đang mở, nên tab đọc là "Timestamp" chứ không phải
"Tools". Ba tab Tools mở cùng lúc mà phân biệt được là lý do duy nhất cần cái này.

## 2. Trạng thái tab

```ts
export interface ToolsTabState {
  toolId: string;
}
```

Đúng luật "ids only" — `toolId` *là* một id, và không có gì khác đi cùng nó. Nội dung hai ô không
được lưu: đây là `localStorage`, và người ta dán token, chuỗi kết nối có mật khẩu, dữ liệu thật vào
đây suốt.

`parseToolsTabState` kiểm shape và chỉ shape: chuỗi, khác rỗng. Việc tool đó còn tồn tại hay không
là câu hỏi của `ToolsTab` lúc mount — giống hệt cách `RestTab` kiểm lại `activeId` vì request có thể
đã bị xoá. Một tool bị gỡ khỏi registry giữa hai lần chạy app thì tab mở tool đầu danh sách, không
báo lỗi.

Đọc `restored` **một lần, trong `useState` initializer**. Gọi `onStateChange` **từ handler chọn
tool**, không từ effect.

## 3. Giai đoạn 1 — bộ khung và năm tool

Năm tool này được chọn vì gần như **không thêm dependency nào**: cái nào cũng là hàm thuần cộng
những gì nền tảng đã cho. Mục đích của giai đoạn là chứng minh bộ khung bằng tool thật chứ không
bằng placeholder.

### 3.1 Timestamp (`time`)

Một ô vào nhận: unix giây, unix mili, unix micro, hoặc chuỗi ISO 8601. **Tự đoán** đơn vị theo độ
lớn — 10 chữ số là giây, 13 là mili, 16 là micro — và nói ra nó đoán gì, vì đoán im lặng mà sai thì
người dùng không có cách nào biết. Có nút "bây giờ".

Ra: ISO UTC, ISO theo múi giờ đã chọn, unix giây, unix mili, và khoảng cách tương đối ("3 ngày
trước"). Danh sách múi giờ lấy từ `Intl.supportedValuesOf("timeZone")`, mặc định là múi của máy.

Logic thuần trong `parse.ts`: `detectUnit(input)` và `toOutputs(instant, tz)`.

### 3.2 Encode & Hash (`encode`)

Một Panel, bốn thẻ con:

| Thẻ | Nội dung | Nền tảng cho sẵn |
| --- | --- | --- |
| Base64 | text ⇄ base64, có biến thể url-safe | `atob`/`btoa` + `TextEncoder` cho UTF-8 |
| Hex | text ⇄ hex, có tuỳ chọn dấu cách | không |
| URL | `encodeURIComponent` và `encodeURI`, cả hai chiều | có |
| Hash | SHA-1/256/384/512, và MD5 | `crypto.subtle.digest` — trừ MD5 |

`atob` hỏng với ký tự ngoài Latin-1, nên đường base64 phải đi qua `TextEncoder`/`TextDecoder` chứ
không gọi thẳng — dán tiếng Việt vào là thấy ngay.

MD5 thì `crypto.subtle` cố tình không có, và nó vẫn cần vì `MD5()` của MySQL và checksum của mọi bản
tải về đều là nó. Tự viết trong `md5.ts` (~70 dòng), test bằng bộ vector chuẩn của RFC 1321. Không
kéo thư viện về cho một hàm băm 70 dòng.

### 3.3 JWT (`encode`)

Tách ba phần, base64url-decode header và payload, in ra bằng `JsonView` đã có. Dưới payload, dịch
`exp`/`iat`/`nbf` thành giờ đọc được và nói token **còn hạn hay đã hết**, vì đó là câu hỏi thật sự
mỗi lần người ta dán một token vào đây.

Chữ ký chỉ hiển thị nguyên trạng, kèm một dòng nói rõ là **không kiểm chứng**. Xem phi mục tiêu.

### 3.4 Sinh ID (`data`)

UUID v4 (`crypto.randomUUID`), UUID v7, ULID, NanoID. Ba cái sau tự viết — mỗi cái vài chục dòng
trên `crypto.getRandomValues`, và v7 với ULID còn phải test được tính **tăng dần theo thời gian**,
thứ mà một thư viện ngoài không cho ta thấy.

Chọn số lượng 1–1000, ra một danh sách mỗi dòng một id, một nút copy tất cả.

### 3.5 Đổi kiểu chữ (`text`)

camelCase, snake_case, kebab-case, PascalCase, CONSTANT_CASE, dot.case, Title Case.

Vào **nhiều dòng, ra nhiều dòng, đổi từng dòng một** — vì cách dùng thật là copy cả danh sách cột từ
tab db rồi dán vào đây, không phải gõ một từ.

Phần khó nằm ở tách từ, và nó là một hàm thuần đáng test kỹ: `getHTTPResponse` phải ra
`get_http_response` chứ không phải `get_h_t_t_p_response`, và `user2FA` phải ra `user_2fa`.

## 4. Giai đoạn 2 — SQL sang MongoDB

Tool duy nhất trong module xứng đáng có mục riêng, và tool duy nhất có rủi ro thật.

### 4.1 Điều phải nói trước

Đây là **bộ dịch best-effort, không phải trình biên dịch.** SQL sang MongoDB không phải ánh xạ toàn
phần. Cái nguy hiểm không phải là dịch không được — mà là **dịch sai mà im lặng**, rồi có người copy
chạy trên production.

Hai luật, và mọi thứ trong mục này phục vụ chúng:

- **Không bao giờ xuất kết quả một phần.** Gặp một mệnh đề không dịch được thì cả câu không có đầu
  ra, và chỗ gây ra được chỉ đích danh. Một truy vấn thiếu mất `HAVING` trông y hệt một truy vấn
  đúng.
- **Khác nghĩa thì phải nói.** Có những chỗ dịch được nhưng ngữ nghĩa Mongo không trùng SQL. Chúng
  vẫn ra kết quả, kèm cảnh báo. Xem 4.5.

### 4.2 Phạm vi

Mức B: `SELECT` thành `find()` và thành aggregation pipeline.

**Dịch được:**

| SQL | MongoDB |
| --- | --- |
| `SELECT a, b FROM t` | `find({}, { a: 1, b: 1, _id: 0 })` |
| `SELECT a AS x` | projection `{ x: "$a" }` |
| `= != <> > >= < <=` | `$eq $ne $gt $gte $lt $lte` |
| `AND` / `OR` / `NOT` | `$and` / `$or` / `$nor` |
| `IN` / `NOT IN` | `$in` / `$nin` |
| `BETWEEN a AND b` | `{ $gte: a, $lte: b }` |
| `IS NULL` / `IS NOT NULL` | `null` / `{ $ne: null }` |
| `LIKE` / `ILIKE` | `$regex` — xem 4.4 |
| `ORDER BY a DESC` | `.sort({ a: -1 })` |
| `LIMIT n OFFSET m` | `.limit(n).skip(m)` |
| `DISTINCT a` | pipeline `$group` |
| `GROUP BY` + `COUNT/SUM/AVG/MIN/MAX` | pipeline `$group` |
| `HAVING` | `$match` **sau** `$group` |

**Từ chối, có tên riêng cho từng cái:** JOIN, subquery, `UNION`, CTE (`WITH`), window function
(`OVER`), `INSERT`/`UPDATE`/`DELETE`, `CASE`, hàm vô hướng (`CONCAT`, `DATE_FORMAT`, …), nhiều câu
lệnh trong một lần dán.

### 4.3 Đường đi

Parser là `node-sql-parser` — thuần JS, có sẵn dialect MySQL và PostgreSQL, ra AST. Nó là thứ nặng
duy nhất của tool, nên `await import("node-sql-parser")` nằm trong hàm dịch, chạy lần đầu người ta
bấm dịch. **Cần đo kích thước bundle thật trước khi chốt**; nếu quá đắt thì phương án hai là duyệt
cây Lezer của `@codemirror/lang-sql`, vốn đã là dependency.

```ts
export type Translation =
  | { ok: true; output: string; warnings: Warning[] }
  | { ok: false; unsupported: Unsupported[] };

export interface Unsupported {
  code: "join" | "subquery" | "union" | "cte" | "window" | "dml" | "case" | "function" | "multi";
  /** Đoạn SQL gây ra, để pane tô đúng chỗ thay vì chỉ nói "không hỗ trợ". */
  fragment: string;
}

export interface Warning {
  code: "isNull" | "type" | "objectId" | "starWithGroupBy";
  /** Trường hoặc đoạn SQL mà cảnh báo nói về. */
  fragment: string;
}
```

`Unsupported` và `Warning` cùng mang `fragment` nhưng nằm ở hai nhánh khác nhau của `Translation`, và
đó là chủ ý: cái thứ nhất *thay cho* đầu ra, cái thứ hai *đi kèm* đầu ra. Gộp hai thứ vào một danh
sách là bước đầu để một ngày nào đó xuất kết quả một phần.

Chọn đường ra: có `GROUP BY`, `HAVING`, `DISTINCT`, hoặc bất kỳ hàm gộp nào thì đi pipeline; còn lại
đi `find()`. `find()` được ưu tiên vì nó ngắn hơn và đọc được — một pipeline `$match` một tầng là
câu trả lời đúng nhưng không phải câu trả lời hữu ích.

Thứ tự stage của pipeline: `$match` (từ `WHERE`) → `$group` → `$match` (từ `HAVING`) → `$project` →
`$sort` → `$skip` → `$limit`. Hai `$match` ở hai vị trí khác nhau chính là chỗ `WHERE` khác `HAVING`,
và đặt sai thì kết quả sai mà không ai thấy.

Đầu ra là cú pháp **mongosh**, in xuống dòng thụt lề, dán thẳng vào Mongo workspace của standalone-client được.

### 4.4 Hai chỗ tinh

**`LIKE` sang `$regex`.** `%` thành `.*`, `_` thành `.`, và **mọi ký tự đặc biệt của regex trong phần
còn lại phải được escape** — `WHERE name LIKE 'a.b%'` mà quên escape dấu chấm thì thành một truy vấn
khác hẳn, vẫn chạy, vẫn ra kết quả. Neo `^` và `$` khi mẫu không mở đầu/kết thúc bằng `%`.

Phân biệt hoa thường thì phụ thuộc dialect: `LIKE` của MySQL không phân biệt (theo collation mặc
định) nên thêm `$options: "i"`; `LIKE` của PostgreSQL có phân biệt, còn `ILIKE` thì không. Đây là lý
do ô chọn dialect có ảnh hưởng thật chứ không chỉ để parse.

**`COUNT`.** `COUNT(*)` là `{ $sum: 1 }`. `COUNT(col)` thì **không** — SQL bỏ qua NULL, nên nó là
`{ $sum: { $cond: [{ $eq: ["$col", null] }, 0, 1] } }`. Dịch cả hai thành `$sum: 1` là loại lỗi chạy
êm và ra số sai.

Sau `$group`, khoá gộp nằm ở `_id`, nên cần một `$project` trả tên cột về như trong `SELECT`.

### 4.5 Cảnh báo

Dịch được nhưng ngữ nghĩa lệch — vẫn ra kết quả, kèm một dòng nói rõ:

- **`IS NULL`.** `{ a: null }` trong Mongo khớp cả tài liệu **thiếu hẳn** trường `a`, còn SQL thì cột
  luôn tồn tại. Muốn đúng nghĩa "có trường và bằng null" thì phải `{ a: { $type: "null" } }`. Tool
  xuất bản thứ nhất và nói ra sự khác biệt, vì trong Mongo cái người ta thường muốn là bản thứ nhất.
- **Kiểu.** `WHERE id = '5'` khớp chuỗi `"5"` chứ không khớp số `5`; SQL thì ép kiểu giúp.
- **`_id` và ObjectId.** Cột tên `_id` so với một chuỗi 24 ký tự hex thì gần như chắc chắn cần
  `ObjectId("...")`. Tool xuất kèm gợi ý đó.
- **`SELECT *` với `GROUP BY`.** MySQL cho qua, Mongo thì không có khái niệm tương ứng.

### 4.6 Kiểm thử

Đây là tool test được kỹ nhất module: `translate(sql, dialect)` là hàm thuần, vào chuỗi ra chuỗi. Bộ
test là một bảng case — mỗi dòng một câu SQL và truy vấn Mongo mong đợi — phủ từng dòng của bảng ở
4.2, từng mã trong `Unsupported`, và từng cảnh báo ở 4.5. Không cần jsdom, không cần server.

## 5. Giai đoạn 3 và 4

Chỉ phác thảo. Mỗi giai đoạn có spec riêng khi tới lượt.

**Giai đoạn 3 — chuyển đổi và văn bản.** Cùng một dạng UI (ô vào → ô ra) và cùng một đợt dependency.

- **Format / Minify.** SQL dùng `sql-formatter` đã có; JSON tự viết; JS/TS/CSS/HTML/MD/YAML dùng
  prettier standalone `import()` động theo ngôn ngữ. **Minify chỉ cho JSON/SQL/XML** — bỏ khoảng
  trắng, gần như miễn phí. Minify JS/CSS cần esbuild-wasm hoặc terser: nặng nhất, giá trị thấp nhất,
  và ai cũng đã có bundler rồi.
- **Chuyển đổi dữ liệu.** JSON ⇄ YAML ⇄ CSV ⇄ `INSERT`, và JSON → `CREATE TABLE` / TypeScript
  interface / Go struct.
- **`.env`** ⇄ JSON ⇄ `export` ⇄ `docker -e`.
- **Diff** hai đoạn text hoặc JSON.
- **Regex tester** có tô match và nhóm bắt.

**Giai đoạn 4 — kết nối và hạ tầng.** Giai đoạn duy nhất chạm Rust, và duy nhất cần lưu trữ. Để cuối
vì rủi ro cao nhất, không phải vì kém giá trị.

- **Kill port.** Backend `tools_scan_port` **chỉ đọc** — `lsof`/`netstat`/`Get-NetTCPConnection` —
  trả PID, tên tiến trình, đường dẫn. Rồi in lệnh kill cho macOS/Linux/Windows để copy. Chọn OS bằng
  tay là có chủ đích: người ta ngồi Windows và cần kill port trên server Linux đang mở ở tab terminal
  bên cạnh.
- **Cheatsheet có tham số.** Chỗ để kill port sống chung: `docker system prune`, `mysqldump`,
  `pg_restore`, `systemctl`… có ô điền biến rồi copy. Cần store riêng của module.
- **Chuỗi kết nối** ⇄ các trường ⇄ JDBC / `.env` / biến docker.

## 6. Thứ tự làm

| GĐ | Nội dung | Dependency mới | Rust |
| --- | --- | --- | --- |
| 1 | Bộ khung + 5 tool (mục 3) | không | không |
| 2 | SQL → MongoDB (mục 4) | `node-sql-parser` | không |
| 3 | Chuyển đổi và văn bản | prettier, js-yaml, diff | không |
| 4 | Kết nối và hạ tầng | không | có |

Giai đoạn 1 ship được một mình. Giai đoạn 2 xếp ngay sau bộ khung thay vì cuối hàng, vì nó là lý do
module này được đặt hàng.

## 7. Việc ngoài `src/modules/tools/`

Đúng những chỗ này và không chỗ nào khác:

- `src/shell/registry.ts` — một dòng trong `MODULES`.
- `src/i18n/dicts.ts` — một cặp import, bốn chỗ spread, và bốn term nữa trong `Collision`.

  **Nhóm i18n của module là `toolbox`, không phải `tools`.** Module `db` đã sở hữu nhóm `tools` —
  `tools.title`, `tools.download`, … cho phần tải `pg_dump`/`mysqldump` trong Settings — và spread
  thứ hai sẽ nuốt trọn nhóm thứ nhất, mất hai chục khoá mà không ai thấy. Id module vẫn là `tools`
  và tên hiển thị vẫn là "Công cụ"; chỉ namespace i18n đổi. Lưới kiểu `Collision` trong `dicts.ts`
  là thứ bắt được chuyện này, và nó chỉ bắt được nếu module mới được nối vào — "module thứ ba thêm
  ba term", nên module thứ tư thêm bốn.

- `src/icons/icons.tsx` và `src/icons/index.ts` — thêm `ToolsIcon`.
- `src/i18n/{en,vi}.ts` — khoá `app.moduleTools`.
- `CHANGELOG.md` — một dòng dưới `## [Unreleased]` mỗi giai đoạn.

`npm run lint` là thứ nói không nếu có file thứ bảy — nó phủ mọi `.ts`/`.tsx` dưới `src/components`,
`src/core`, `src/icons`, `src/shell` và `src/i18n`, và `registry.ts` với `dicts.ts` là hai ngoại lệ
duy nhất.
