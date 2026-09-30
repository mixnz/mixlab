---
status: implemented
date: 2026-08-22
---

# Lưới kết quả của tab Query

Ngày: 2026-08-22

## Mục tiêu

Kết quả của một `SELECT` trong tab Query đọc được và lấy ra được, chứ không chỉ nhìn được.

Sau khi làm xong:

- Bấm vào tiêu đề cột thì **sắp xếp** những dòng đã trả về, theo cột đó, xuôi rồi ngược rồi thôi.
- Một ô lọc phía trên lưới **giữ lại những dòng khớp** và nói còn bao nhiêu trên bao nhiêu.
- Click chọn một ô, Shift+click kéo ra **một vùng chữ nhật**, `Ctrl+A` chọn hết, `Ctrl+C` chép ra
  TSV. Chuột phải cho chọn TSV / CSV / JSON, cho vùng đang chọn hoặc cho cả kết quả.
- Double-click hoặc `Enter` **mở rộng một ô** ra một dialog đọc được — JSON tô màu nếu là JSON.
- Trên đầu pane kết quả có một dòng: **`3 / 5 câu lệnh · 812 ms`**, để một script bị chặn giữa
  chừng không bị đọc nhầm thành một script chạy xong.

## Phi mục tiêu

Ghi ra để không bị kéo vào:

- **Không đụng vào selection của [`SqlTable`](../../apps/desktop/src/modules/db/components/SqlTable/SqlTable.tsx).**
  Nó chọn theo **dòng** (`selectedRows: Set<number>`, `anchorRowRef`) vì mọi việc nó làm sau đó —
  chép ra `INSERT`, xoá, clone — là việc trên cả dòng. Lưới này chọn theo **ô** vì ở đây không có
  dòng nào có danh tính: một `SELECT` tuỳ ý không có khoá chính, không có bảng để ghi lại, và thứ
  người ta lấy ra thường là ba cột trong hai mươi. Hai mô hình khác nhau vì hai việc khác nhau, và
  gộp chúng lại không nằm trong đợt này.
- **Không ẩn cột.** Có trong roadmap, không có trong phạm vi đã chốt.
- **Không xuất ra file.** Chỉ chép vào clipboard. Ghi file cần dialog lưu của Tauri, và đó là một
  việc riêng có bề mặt riêng.
- **Không xuất ra `INSERT`.** Không có tên bảng để `INSERT INTO` cái gì. `SqlTable` làm được vì nó
  biết mình đang xem bảng nào.
- **Không sắp xếp phía máy chủ.** Sort ở đây chỉ sắp lại **những dòng đã về**. Xem mục Rủi ro.
- **Không sửa ô.** Kết quả của một script không phải là dữ liệu có đường về.

## Hiện trạng

| Chỗ | Sự thật |
| --- | --- |
| [`ResultGrid.tsx`](../../apps/desktop/src/modules/db/components/QueryEditor/ResultGrid.tsx) | 228 dòng. Props đúng ba thứ: `columns`, `rows`, `emptyLabel`. Không selection, không sort, không copy |
| [`ResultGrid.tsx:116`](../../apps/desktop/src/modules/db/components/QueryEditor/ResultGrid.tsx#L116) | Rows là **positional** `unknown[][]`, cố ý: một `SELECT` được phép đặt trùng tên cột hai lần, nên `<th key={c}>` cũng đánh theo vị trí |
| [`ResultGrid.tsx:90`](../../apps/desktop/src/modules/db/components/QueryEditor/ResultGrid.tsx#L90) | `ResultRow` memo theo `row` / `index` / `columns`. Comment ghi rõ: mất memo này thì mỗi lần cửa sổ trượt là dựng lại cả nghìn ô |
| [`ResultGrid.tsx:148`](../../apps/desktop/src/modules/db/components/QueryEditor/ResultGrid.tsx#L148) | `measureLayout` chạy trong `useLayoutEffect` phụ thuộc `[virtual, columns, rows]` — đo bề rộng cột từ **toàn bộ** rows |
| [`ResultGrid.tsx:22`](../../apps/desktop/src/modules/db/components/QueryEditor/ResultGrid.tsx#L22) | `VIRTUAL_FROM = 60`: từ 60 dòng trở lên thì `tbody` chỉ giữ cửa sổ đang nhìn thấy |
| [`QueryResults.tsx:55-64`](../../apps/desktop/src/modules/db/components/QueryEditor/QueryResults.tsx#L55-L64) | Dòng `limitAdded` nằm **ngoài** `.results`, kèm comment giải thích: một dòng chữ chung cột cuộn với card đầu tiên sẽ đẩy đáy card ra khỏi pane |
| [`rowText.ts:172-201`](../../apps/desktop/src/modules/db/components/SqlTable/rowText.ts#L172-L201) | `delimitedCell` / `csvText` / `spreadsheetText` nhận `Record<string, unknown>` — không dùng lại thẳng cho row positional được |
| [`rowText.ts:1-7`](../../apps/desktop/src/modules/db/components/SqlTable/rowText.ts#L1-L7) | Comment đầu file: phần escape là "thứ dễ sai âm thầm và không thể thấy sai trên màn hình". Đó là lý do không nhân bản nó |
| [`QueryEditor.tsx:412`](../../apps/desktop/src/modules/db/components/QueryEditor/QueryEditor.tsx#L412) | `requestRun` đã có `statements` trong tay trước khi gọi `run()` — **M** của dòng tóm tắt lấy ở đây |
| [`mysql_script.rs:320`](../../apps/desktop/src-tauri/src/modules/db/drivers/mysql_script.rs#L320) | Một câu lệnh lỗi **dừng cả script**. Nên `results.length` có thể nhỏ hơn số câu lệnh đã gửi, và không có gì trên màn hình nói ra điều đó — đây là lý do dòng tóm tắt tồn tại |

## Quyết định nền: view là một mảng chỉ số

Sort và find **không sinh mảng row mới**. Chúng sinh `number[]` — thứ tự các chỉ số của row gốc.
Lưới đọc `rows[view[i]]`.

Đây là quyết định chi phối cả phần còn lại, vì ba thứ đã có sẵn đều gãy nếu làm cách khác:

- **Đo cột.** `measureLayout` phụ thuộc `[virtual, columns, rows]`. Một mảng copy mới sau mỗi ký tự
  gõ vào ô lọc là đo lại toàn bộ cột sau mỗi ký tự.
- **Memo từng dòng.** `ResultRow` memo theo identity của `row`. Mảng chỉ số giữ nguyên identity của
  từng row, nên lọc xong chỉ những dòng thật sự đổi chỗ mới phải dựng lại.
- **Cột `#`.** Nó phải nói vị trí **gốc** của dòng: sort xong vẫn biết dòng này là dòng thứ mấy
  trong kết quả thật. Có mảng chỉ số thì đó là `view[i] + 1`, không cần gì thêm.

Ba module thuần, không cái nào biết React, mỗi cái một file test:

| File | Nội dung |
| --- | --- |
| `src/core/gridText.ts` *(mới)* | `tsvText` / `csvText` / `jsonText` trên `unknown[][]` |
| `QueryEditor/resultView.ts` *(mới)* | `viewIndexes`, `nextSort`, `compareValues`, `rowMatches` |
| `QueryEditor/resultSelection.ts` *(mới)* | reducer thuần trên vùng chọn, và cắt vùng đó ra `unknown[][]` |

## 1. `src/core/gridText.ts`

Đặt cạnh [`virtualRows.ts`](../../apps/desktop/src/core/virtualRows.ts) và
[`clipboard.ts`](../../apps/desktop/src/core/clipboard.ts) — cùng một hạng: chuyện của lưới nói chung, không
có khái niệm nào của module db trong đó.

```ts
export function tsvText(columns: string[], rows: unknown[][]): string;
export function csvText(columns: string[], rows: unknown[][]): string;
export function jsonText(columns: string[], rows: unknown[][]): string;
export function uniqueNames(columns: string[]): string[];
```

`delimitedCell`, `TAB_SEPARATOR`, `COMMA_SEPARATOR`, `ROW_SEPARATOR` **chuyển nguyên vẹn** từ
[`rowText.ts`](../../apps/desktop/src/modules/db/components/SqlTable/rowText.ts) xuống đây, kèm nguyên các
comment giải thích vì sao CRLF và vì sao NULL thành ô rỗng. Không sửa một ký tự nào của phần escape.

`rowText.ts` giữ nguyên API cũ: `spreadsheetText` và `csvText` của nó thành wrapper mỏng, map
`Record<string, unknown>` sang mảng theo `columns` rồi gọi xuống. `quoteIdentifier`, `sqlLiteral`,
`insertStatements` ở nguyên chỗ cũ — chúng là MySQL và chỉ tab Data dùng.

[`rowText.test.ts`](../../apps/desktop/src/modules/db/components/SqlTable/rowText.test.ts) hiện có **là lưới
an toàn của bước dời này**: nó phải xanh mà không sửa một dòng nào. Nếu phải sửa nó, tức là bước dời
đã đổi hành vi, và đó là lỗi chứ không phải là cập nhật.

`jsonText` xuất một mảng object. Tên cột trùng nhau thì `uniqueNames` thêm hậu tố: `id`, `id_2`,
`id_3`. JSON không có chỗ nào khác để giấu chuyện hai cột cùng tên, và bỏ im lặng một trong hai là
cách tệ nhất trong ba cách.

## 2. `resultView.ts` — sort và find

```ts
export interface Sort { column: number; direction: "asc" | "desc" }

export function compareValues(a: unknown, b: unknown): number;
export function rowMatches(row: unknown[], needle: string): boolean;
export function nextSort(current: Sort | null, column: number): Sort | null;
export function viewIndexes(rows: unknown[][], sort: Sort | null, query: string): number[];
```

**`compareValues`**: `null`/`undefined` luôn xuống cuối, ở cả hai chiều — một cột toàn NULL nằm đầu
danh sách giảm dần là thứ không ai muốn nhìn. Hai số so theo số, hai `bigint` so theo `bigint`. Còn
lại `String()` rồi `localeCompare` với `numeric: true`, để `item2` đứng trước `item10`.

**`rowMatches`**: so trên chính chuỗi mà lưới hiển thị — `displayValue(row[c])` của
[`virtualRows.ts`](../../apps/desktop/src/core/virtualRows.ts) — không phân biệt hoa thường, khớp chuỗi con,
không regex. Người ta gõ vào đây một cái id hoặc một mẩu email, không gõ regex.

**`nextSort`**: cùng cột thì `asc` → `desc` → `null`. Cột khác thì bắt đầu lại ở `asc`.

**`viewIndexes`**: lọc trước, sắp sau (sắp một tập nhỏ hơn), sort **ổn định** bằng cách so chỉ số
gốc khi hai giá trị bằng nhau. Không sort và không lọc thì trả về mảng đồng nhất.

## 3. `resultSelection.ts` — vùng chọn

Toạ độ ở đây là toạ độ **view**: dòng thứ mấy trên màn hình, cột thứ mấy. Không phải chỉ số row gốc.

```ts
export interface Cell { row: number; col: number }
export interface Selection { anchor: Cell; focus: Cell }
export interface Rect { top: number; left: number; bottom: number; right: number }

export function moveSelection(current: Selection | null, cell: Cell, extend: boolean): Selection;
export function selectAll(rowCount: number, columnCount: number): Selection | null;
export function rectOf(selection: Selection | null): Rect | null;
export function spanIn(rect: Rect | null, viewRow: number): [number, number];
export function cutOut(
  rect: Rect,
  view: number[],
  rows: unknown[][],
  columns: string[]
): { columns: string[]; rows: unknown[][] };
```

`moveSelection` với `extend` là Shift+click: giữ `anchor`, dời `focus`. Không có Ctrl+click gộp
nhiều vùng rời — một vùng chữ nhật là thứ dán được vào bảng tính, nhiều vùng rời thì không.

**`spanIn` là chỗ giữ cho memo không gãy.** Nó trả về `[from, to]` — hai **số** — hoặc `[-1, -1]`
khi dòng đó không dính gì tới vùng chọn. `ResultRow` nhận hai số này chứ không nhận một object: một
object mới ở mỗi lần render sẽ phá memo của cả sáu mươi dòng đang hiện, mỗi lần chuột nhích một ô.

`cutOut` cắt vùng chọn ra thành `{ columns, rows }` đúng shape mà `gridText` nhận. Đây là chỗ duy
nhất toạ độ view được đổi ngược về row gốc.

## 4. `ResultGrid.tsx`

State thêm vào: `sort`, `query` (chuỗi trong ô lọc), `selection`, `expanded` (ô đang mở rộng),
`menu` (vị trí chuột phải).

- `view = useMemo(() => viewIndexes(rows, sort, query), [rows, sort, query])`.
- `measureLayout` **vẫn phụ thuộc `rows`, không phụ thuộc `view`** — cột đo theo dữ liệu, không
  theo cái đang được nhìn. Lọc rồi mà cột co lại là lưới nhảy dưới tay người đang gõ.
- `useVirtualRows` nhận `total = view.length`.
- Cột `#` in `view[i] + 1`.
- `<th>` bấm được: `onClick` gọi `nextSort`, mũi tên trong header, `aria-sort` theo chuẩn.
- **Vùng chọn bị xoá khi `sort`, `query`, `columns` hoặc `rows` đổi.** Giữ một hình chữ nhật ở
  nguyên toạ độ cũ trong khi các dòng vừa đổi chỗ dưới chân nó thì tệ hơn là mất nó.
- Bàn phím trên `.gridWrap` (`tabIndex={0}`): `Ctrl+A` chọn hết, `Ctrl+C` chép TSV, `Enter` mở rộng
  ô đang focus, `Escape` bỏ chọn, phím mũi tên dời ô (Shift+mũi tên nới vùng).
- Chuột phải mở [`ContextMenu`](../../apps/desktop/src/components/ContextMenu.tsx) như
  [`SqlTable`](../../apps/desktop/src/modules/db/components/SqlTable/SqlTable.tsx#L1602) đang làm: nếu ô dưới
  chuột nằm ngoài vùng đang chọn thì vùng chọn thu về ô đó trước, đúng cách `SqlTable` xử lý.

Thanh find là một hàng mảnh trên lưới, **chỉ hiện từ `FIND_FROM = 20` dòng trở lên**. Dưới mức đó
mọi thứ đã nằm trên màn hình và ô lọc chỉ là đồ đạc. Bên phải thanh là "còn N / M dòng", chỉ khi
đang lọc.

Ba tệp mới trong thư mục: `resultView.ts`, `resultSelection.ts`, `CellDialog.tsx`. `ResultGrid.tsx`
tự nó lên khoảng 380–420 dòng — chấp nhận được vì phần logic đã ra ngoài, phần còn lại là JSX và
handler.

## 5. `CellDialog.tsx`

Dialog đọc một ô. Dùng [`JsonView`](../../apps/desktop/src/components/JsonView/JsonView.tsx) khi giá trị là
object, hoặc là chuỗi mà `JSON.parse` nuốt được và cho ra object/array; còn lại là `<pre>` chọn được
chữ. Header nói tên cột và số dòng, có nút chép, đóng bằng `Escape` hoặc bấm ra ngoài. Theo đúng
`dialogMotion` mà các dialog khác trong app đang dùng.

## 6. Dòng tóm tắt

`QueryResults` nhận thêm một prop: `statementsSent: number`. `QueryEditor` giữ nó trong state, đặt
trong `run()` từ `statements.length` — con số nó đã có ở
[dòng 412](../../apps/desktop/src/modules/db/components/QueryEditor/QueryEditor.tsx#L412).

Dòng này đặt **cạnh dòng `limitAdded`**, ngoài `.results`, đúng vì lý do comment ở
[`QueryResults.tsx:58-61`](../../apps/desktop/src/modules/db/components/QueryEditor/QueryResults.tsx#L58-L61)
đã ghi.

Chỉ hiện khi `statementsSent > 1`. Một câu lệnh thì thời gian của nó đã nằm trong header của chính
card đó, và dòng này không thêm gì.

```
5 câu lệnh · 812 ms          ← results.length === statementsSent
3 / 5 câu lệnh · 812 ms      ← script bị một câu lệnh chặn lại
```

`812 ms` là **tổng `durationMs` của các câu lệnh đã chạy**, đo phía máy chủ. Không phải thời gian
người dùng chờ: chênh lệch là round trip và giải mã. Các con số trong header cộng lại đúng bằng con
số này, và đó là thứ làm cho dòng này đọc được.

## 7. i18n

Thêm vào `query.*` của [`en.ts`](../../apps/desktop/src/modules/db/i18n/en.ts) và
[`vi.ts`](../../apps/desktop/src/modules/db/i18n/vi.ts):

| Khoá | en |
| --- | --- |
| `scriptSummary` | `{{n}} of {{m}} statements · {{ms}} ms` |
| `scriptSummaryAll` | `{{m}} statements · {{ms}} ms` |
| `sortAsc` / `sortDesc` / `sortNone` | tooltip của header, nói rõ chỉ sắp những dòng đã trả về |
| `findPlaceholder` | `Filter these rows...` |
| `findCount` | `{{n}} of {{m}} rows` |
| `noMatchingRows` | `No row here matches that.` |
| `copySelection` / `copySelectionCsv` / `copySelectionJson` | vùng đang chọn |
| `copyAll` / `copyAllCsv` / `copyAllJson` | cả kết quả |
| `expandCell` | `Open this cell` |
| `cellTitle` | `{{column}}, row {{n}}` |

## Rủi ro và những chỗ dễ sai

- **Sort chỉ sắp những dòng đã về.** Một kết quả bị cắt ở 1000 dòng, hoặc bị `LIMIT` mà the standalone client tự
  thêm, sắp giảm dần **không** cho ra giá trị lớn nhất của bảng. Tooltip của header nói thẳng câu
  đó. Đây là hiểu nhầm dễ xảy ra nhất trong cả đợt này.
- **Kết quả rỗng sau khi lọc.** `emptyLabel` hiện tại nói "The result set is empty", sai khi thật ra
  là bộ lọc đã cắt hết. Hai câu phải khác nhau — đó là `noMatchingRows`.
- **Đo cột theo `rows` chứ không theo `view`** — đã nói ở mục 4, và là lỗi hiệu năng sẽ không lộ ra
  trên kết quả nhỏ.
- **`Ctrl+A` và `Ctrl+C` phải chỉ ăn khi lưới đang có focus**, không cướp phím của editor SQL ngay
  phía trên. `ContextMenu` đã tự gọi `enterModal()` nên phần đó không cần lưới lo.
- **Chép một kết quả 1000 dòng × 40 cột** là vài megabyte chuỗi dựng đồng bộ. Chấp nhận: nó chỉ chạy
  khi người dùng chủ động chọn, và trần 1000 dòng của backend đã chặn đầu trên.

## Kiểm chứng

`npm test` cho ba module thuần, và cho `rowText.test.ts` cũ **không sửa**. `npm run build`.

**Không tự kiểm được, phải chạy `npm run dev:app`:** kéo chọn vùng bằng chuột, context menu, dialog
mở rộng ô, cảm giác gõ trong ô lọc trên kết quả lớn, và sort trên một kết quả đã virtual hoá. Những
thứ này sẽ được liệt kê lại ở cuối, không được báo là đã kiểm.

## Thứ tự làm

1. `src/core/gridText.ts` + test; `rowText.ts` gọi xuống nó; `rowText.test.ts` phải xanh y nguyên.
2. `resultView.ts` + test.
3. `resultSelection.ts` + test.
4. Dòng tóm tắt `N / M câu lệnh · ms` — độc lập với lưới, làm sớm để có cái chạy được.
5. Sort trong `ResultGrid` (mảng `view`, header bấm được, cột `#` theo chỉ số gốc).
6. Thanh find.
7. Vùng chọn + `Ctrl+A` / `Ctrl+C` + context menu chép TSV/CSV/JSON.
8. `CellDialog.tsx`.
9. CHANGELOG, và ghi lại vào [`query-editor-roadmap.md`](../roadmap/desktop/query-editor.md)
   những gì còn nợ: ẩn cột, xuất ra file, và việc gộp selection của hai lưới.
