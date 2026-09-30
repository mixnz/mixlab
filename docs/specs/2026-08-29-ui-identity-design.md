---
status: superseded
date: 2026-08-29
superseded_by: 2026-09-17-t157-mixlab-redesign-design.md
---

# Bản sắc giao diện: từ "chạy được" sang "có người quyết định"

Ngày: 2026-08-29

## Mục tiêu

The standalone client trông như một công cụ có người chọn cho nó một hình hài, chứ không như một app vừa được
dựng xong và chưa ai ngồi xuống nhìn.

Sau khi làm xong:

- **Chrome dùng sans, dữ liệu dùng mono.** Label, nút, tab, menu, tiêu đề, Settings đọc bằng font
  hệ thống. Lưới, editor, JSON, hex, terminal giữ Fira Code — vì ở đó việc so từng ký tự là thật.
- **Mật độ của một công cụ, không phải của một trang web.** Chrome 13px/18px thay cho 16px/24px, và
  một thang bốn bậc thay cho "mọi thứ xấp xỉ bằng nhau".
- **Lưới hiển thị nhiều dòng hơn.** 33px → 27px một dòng: đo được 21 → 26 dòng ở 1440×900.
- **Màu chủ đạo là một lựa chọn.** `#396cd8` hiện tại là màu mặc định của template
  `create-tauri-app` — nó chưa bao giờ được chọn.
- **Form trông như form.** Không còn `<fieldset>` viền-có-chữ-cắt-ngang mặc định của trình duyệt,
  không còn ô nhập kéo dài 900px.
- **Tab của mỗi tầng trông ra tầng đó.** Vạch accent 2px thôi là tín hiệu duy nhất cho cả bốn tầng
  lồng nhau, và những hàng vốn là *lựa chọn* thôi giả làm tab.

## Phi mục tiêu

Ghi ra để không bị kéo vào:

- **Không bỏ bảng chọn accent.** Cả mười palette ở [`App.css:112-155`](../../apps/desktop/src/shell/App.css#L112-L155)
  ở nguyên, cơ chế ba dạng (`--accent` / `--accent-text` / `--accent-rgb`) ở nguyên, pane Appearance
  vẫn cho người dùng đổi màu. Chỉ **giá trị mặc định** của `--c-blue` thay đổi. Cái làm app trông
  như máy sinh ra không phải là việc có picker — nhiều công cụ thật có — mà là màu mặc định lấy
  nguyên từ scaffold.
- **Không đổi accent sang xanh lá.** Xem "Quyết định: vì sao không phải xanh lá".
- **Không đụng vào layout của bất kỳ module nào.** Splitter, thứ tự pane, cây sidebar giữ nguyên.
  Đây là đợt về **chất liệu** (font, cỡ, mật độ, màu, cách đánh dấu cái đang chọn), không phải về
  bố cục. Phần 6 đổi *hình hài* của tab, không đổi tab nào nằm ở đâu hay chứa gì.
- **Không đụng API của `TabStrip`.** `items` / `activeId` / `onSelect` / `onClose` / `onNew` /
  `size` / `before` giữ nguyên, kể cả knob `--tab-accent`. Phần 6 chỉ đổi CSS phía trong.
- **Không làm lại icon.** Bộ logo bốn DB mỗi cái một style là một vấn đề thật, nhưng là vấn đề khác
  và cần tài sản đồ hoạ, không cần CSS.
- **Không đụng `--danger` và `--readonly`.** Chúng nói lên sự thật về dữ liệu, không phải trang trí.
- **Không thêm theme mới.** Light/Dark/System giữ nguyên ba trạng thái.

## Hiện trạng

| Chỗ | Sự thật |
| --- | --- |
| [`App.css:2`](../../apps/desktop/src/shell/App.css#L2) | `:root` đặt `font-family: "Fira Code", monospace`. Toàn app kế thừa từ đây — kể cả đoạn văn giải thích Liquid glass trong Settings |
| [`App.css:3-4`](../../apps/desktop/src/shell/App.css#L3-L4) | `font-size: 16px`, `line-height: 24px`. Đây là gốc của mọi `em` bên dưới |
| — | 31 chỗ viết thẳng `"Fira Code"` trong CSS module; 4 chỗ dùng `var(--font-mono)` |
| [`HistoryDialog.module.css:183`](../../apps/desktop/src/modules/rest/components/HistoryDialog/HistoryDialog.module.css#L183) | `font-family: var(--font-mono)` — **token này chưa bao giờ được định nghĩa**. Không có fallback, nên rule vô hiệu và chữ kế thừa Fira Code từ `:root`. Nó đúng nhờ tai nạn |
| [`diff/Panel.module.css:55`](../../apps/desktop/src/modules/tools/tools/diff/Panel.module.css#L55), [`regex/Panel.module.css:19`](../../apps/desktop/src/modules/tools/tools/regex/Panel.module.css#L19) | Cùng token chưa định nghĩa, nhưng có `, monospace` đỡ phía sau |
| [`TableStructure.module.css:260`](../../apps/desktop/src/modules/db/components/TableStructure/TableStructure.module.css#L260) | `font-family: system-ui, sans-serif` — chỗ **duy nhất** trong app đã là sans |
| — | 179 khai báo `font-size` theo `em`, 71 theo `rem`, 1 theo `px`. `em` chồng nhau, nên đổi gốc 16px→13px không phải một phép nhân đều |
| [`App.css:112`](../../apps/desktop/src/shell/App.css#L112) | `--c-blue: #396cd8` — đúng giá trị accent mặc định của template `create-tauri-app` |
| [`App.css:109`](../../apps/desktop/src/shell/App.css#L109) | Comment đã ghi luật chọn hue: đạt 4.5:1 trên nền sáng, và **khác với đỏ** vì đỏ nghĩa là xoá — *"which is why there is no red among them"* |
| [`ToolsSection.module.css:143`](../../apps/desktop/src/modules/db/components/ToolsSection/ToolsSection.module.css#L143) | `--c-green-text` = dump xong |
| [`db.css:519`](../../apps/desktop/src/modules/db/db.css#L519) | `.tunnel-status-ok` dùng `--c-green-text` |
| [`TunnelBanner.module.css:48`](../../apps/desktop/src/modules/db/components/TunnelBanner/TunnelBanner.module.css#L48) | `.reconnected` viền trái `#2e7d32` — xanh lá hardcode, không qua palette |
| [`virtualRows.ts:513`](../../apps/desktop/src/core/virtualRows.ts#L513) | `gridStyle(rowHeight, width)` đặt `--row-h` từ **một số JavaScript** |
| [`virtualRows.ts:14-16`](../../apps/desktop/src/core/virtualRows.ts#L14-L16) | Bất biến: mọi dòng cao đúng `rowHeight`, vì dòng ngoài khung được thay bằng spacer `count × rowHeight`. Lệch một chút là đáy trang trôi khi cuộn |
| [`SqlTable.tsx:69`](../../apps/desktop/src/modules/db/components/SqlTable/SqlTable.tsx#L69) | `ROW_HEIGHT = 33`, kèm comment: *"changing the grid's font or padding means changing this with them"* |
| [`ResultGrid.tsx:68`](../../apps/desktop/src/modules/db/components/QueryEditor/ResultGrid.tsx#L68) | `ROW_HEIGHT = 31` — lưới Query, khác 2px với ba lưới kia |
| [`TableStructure.tsx:71`](../../apps/desktop/src/modules/db/components/TableStructure/TableStructure.tsx#L71) | `ROW_HEIGHT = 33`. Comment ghi thêm: nút trong dòng cũng cao 24px |
| [`DatabaseStats.tsx:56`](../../apps/desktop/src/modules/db/components/DatabaseStats/DatabaseStats.tsx#L56) | `ROW_HEIGHT = 33` |
| [`SqlTable.module.css:110-116`](../../apps/desktop/src/modules/db/components/SqlTable/SqlTable.module.css#L110-L116) | Rule ghim dòng: `height: var(--row-h)`, `padding-top/bottom: 0`, `line-height: calc(var(--row-h) - 1px)`. **Padding bị cố tình khử** |
| [`ConnectionForm.tsx:158`](../../apps/desktop/src/modules/db/components/ConnectionForm/ConnectionForm.tsx#L158), [`:252`](../../apps/desktop/src/modules/db/components/ConnectionForm/ConnectionForm.tsx#L252) | Hai `<fieldset>` + `<legend>` không style — viền có chữ cắt ngang là mặc định trình duyệt |
| — | `.login-form` không có `max-width`. Ở 1440px, ô "Host" rộng 470px và form trải 1020px |
| [`TabStrip.module.css:122`](../../apps/desktop/src/components/TabStrip/TabStrip.module.css#L122) | Tab đang mở: `box-shadow: inset 0 2px 0 var(--tab-accent, var(--accent))` — vạch 2px cạnh **trên**. Dùng cho tab cửa sổ, tab request REST và tab pane REST |
| [`db.css:399`](../../apps/desktop/src/modules/db/db.css#L399) | `.method-tab`: `border-bottom: 2px solid transparent` — vạch 2px cạnh **dưới**. Cùng một class phục vụ ba việc khác hẳn nhau: chọn loại DB, chọn TCP/IP\|SSH, và chuyển Data/Structure/Statistics/Query |
| [`SqlWorkspace.tsx:575`](../../apps/desktop/src/modules/db/sql/SqlWorkspace.tsx#L575) | `className="method-tabs sql-content-tabs"` — tab nội dung của workspace mượn thẳng class của cái picker chọn loại DB |
| [`encode/Panel.module.css:38`](../../apps/desktop/src/modules/tools/tools/encode/Panel.module.css#L38) | `box-shadow: inset 0 -2px 0 var(--accent)` — cách viết thứ ba của cùng một vạch |

## Quyết định nền: hai tầng font, và mật độ là một con số khai báo

Hai điều chi phối cả phần còn lại.

**Một.** Font không phải một giá trị, mà là hai vai. `--font-ui` cho thứ app tự nói ra;
`--font-mono` cho thứ đến từ database. Hôm nay chỉ có một vai, nên mọi thứ mang vai đó — và đó là
lý do đoạn văn sáu dòng trong Settings đang được đọc bằng font lập trình.

Ranh giới không phải "chỗ nào trông hợp", mà là: **mono khi việc so từng ký tự là thật.** Một giá
trị `varchar` cần thấy được khoảng trắng cuối; một nhãn "Save connection" thì không.

Hệ quả cho sidebar: **tên bảng và tên collection dùng sans.** Chúng là định danh, nhưng ở đó bạn
đang quét tìm một cái tên, không so ký tự — và sans quét nhanh hơn ở cỡ chữ nhỏ.

**Hai.** Mật độ lưới **không** đi theo font. Vì bất biến ở
[`virtualRows.ts:14`](../../apps/desktop/src/core/virtualRows.ts#L14), chiều cao dòng là một số khai báo trong
TypeScript mà CSS phải khớp lại. Đổi `:root` font-size không làm dòng thấp đi; nó chỉ làm chữ nhỏ
lại trong một dòng vẫn cao 33px.

Đây là chỗ đã bị làm sai một lần trong lúc dựng thử: chỉnh `padding` của ô để "nén dòng lại" khiến
dòng **phồng từ 33px lên 38.8px** và số dòng thấy được **giảm** từ 21 xuống 18 — đúng hỏng mà comment
ở [`SqlTable.module.css:97`](../../apps/desktop/src/modules/db/components/SqlTable/SqlTable.module.css#L97)
mô tả. Padding đá nhau với `height: var(--row-h)`.

Nên mật độ lưới là **một task riêng, có rủi ro riêng**, không phải hệ quả miễn phí của đợt token.

## Quyết định: vì sao không phải xanh lá

Xanh lá đã mang nghĩa trong app này. Nó là màu của *thành công* ở ba chỗ: dump xong
([`ToolsSection.module.css:143`](../../apps/desktop/src/modules/db/components/ToolsSection/ToolsSection.module.css#L143)),
tunnel còn sống ([`db.css:519`](../../apps/desktop/src/modules/db/db.css#L519)),
tunnel vừa nối lại ([`TunnelBanner.module.css:48`](../../apps/desktop/src/modules/db/components/TunnelBanner/TunnelBanner.module.css#L48)).

Accent xanh lá làm nút "Connect" và dòng chữ "tunnel OK" nói cùng một thứ bằng mắt trong khi nghĩa
khác hẳn nhau. Đó chính là lý do palette đã cố tình loại đỏ ra —
[`App.css:109`](../../apps/desktop/src/shell/App.css#L109) ghi rõ. Xanh lá dính cùng cái bẫy, chỉ là chưa ai
để ý vì nó không đứng trong palette với tư cách "màu trạng thái".

Giá trị chốt: **`--c-blue: #23528c`**, cast chữ `#1d4576`, cast tối `#6f9fd8`. Vẫn là xanh dương
như đã chọn, nhưng tối và bớt bão hoà hơn `#396cd8` đủ để không còn ai nhận ra màu scaffold.

## Các phần

### 1. Tầng token font

Thêm vào `:root`:

```css
--font-ui: ui-sans-serif, -apple-system, "Segoe UI Variable Text", "Segoe UI", Roboto, sans-serif;
--font-mono: "Fira Code", ui-monospace, monospace;
```

`:root` chuyển sang `--font-ui`. 31 chỗ hardcode `"Fira Code"` đổi thành `var(--font-mono)` — thuần
thay thế, không đổi hình. Việc này cũng **vá luôn** token chưa định nghĩa ở bốn chỗ đang dùng nó.

Giữ mono: ô dữ liệu của cả bốn lưới, SQL/query editor, JsonView, TreeView, DocumentNode, HexView,
RedisValue, terminal, các Tools panel, UrlBar của REST, giá trị trong CellDialog.

Sang sans: mọi thứ còn lại, kể cả **header của lưới** (`<th>`) — tên cột là nhãn, không phải dữ liệu.

### 2. Thang chữ và mật độ chrome

`:root` sang `13px` / `18px`. Thêm thang bốn bậc:

| Token | Giá trị | Dùng cho |
| --- | --- | --- |
| `--text-xs` | 11px | badge, caption, chú thích |
| `--text-sm` | 12px | label phụ, dòng trạng thái |
| `--text-md` | 13px | thân, mọi control |
| `--text-lg` | 15px | tiêu đề pane, tiêu đề dialog |

Weight: 400 thân, 500 label và nút, 600 tiêu đề.

Đây là phần tốn công nhất. 179 khai báo `em` sẽ dịch chuyển theo gốc, và chúng chồng nhau, nên phải
rà từng màn hình chứ không thể suy ra.

### 3. Bỏ `fieldset`, giới hạn bề ngang form

**Phải nằm cùng đợt với phần 1-2, không để sau.** Khi chrome chuyển sang sans, `fieldset` mặc định
trông *tệ hơn* hiện tại — sans làm lộ ra chúng là HTML thô. Để lại sang đợt sau là chấp nhận một
giai đoạn xấu hơn lúc chưa làm gì.

- `<fieldset>` bỏ viền và padding; `<legend>` thành nhãn nhóm 11px, uppercase, letter-spacing
  `0.07em`, opacity `0.55`.
- `.login-form` nhận `max-width: 620px`. Host/Port vẫn đứng cạnh nhau vừa vặn ở bề ngang này.

### 4. Màu mặc định

Đổi ba giá trị `--c-blue*` ở [`App.css:112-114`](../../apps/desktop/src/shell/App.css#L112-L114) và cast tối
tương ứng ở cuối file. Không đụng chín palette còn lại.

### 5. Mật độ lưới — task riêng

Bốn hằng `ROW_HEIGHT` xuống theo tỉ lệ 33 → 27 (Query grid 31 → 25, giữ nguyên chênh lệch 2px đang
có). CSS ghim dòng đổi cùng lúc trong cùng một commit — hai nơi lệch nhau là scroll drift.

| | Trước | Sau |
| --- | --- | --- |
| Chiều cao dòng | 33px | 27px |
| Dòng thấy được @1440×900 | 21 | 26 |

Đo bằng cách tiêm CSS lúc chạy, không phải ước lượng.

### 6. Tab: một tín hiệu đang phải gánh bốn tầng

Vạch accent 2px là **thứ duy nhất** app dùng để nói "cái này đang được chọn", ở bốn tầng lồng nhau,
trên hai cạnh đối nhau mà sự khác nhau đó không mang nghĩa gì. Khi một tín hiệu đánh dấu mọi tầng,
nó thôi xếp hạng được tầng nào — nhìn vạch xanh không biết đang ở tab cửa sổ hay ở một picker con.

Và có một lỗi ngữ nghĩa nằm dưới lỗi thẩm mỹ: **MySQL|PostgreSQL|MongoDB|Redis và TCP/IP|SSH không
phải tab.** Chúng là lựa chọn *về một form* — đổi chúng đổi các ô bên dưới, không đưa bạn sang một
view khác. Đang trông y hệt Data/Structure/Query là nói sai về việc chúng làm.
[`SqlWorkspace.tsx:575`](../../apps/desktop/src/modules/db/sql/SqlWorkspace.tsx#L575) cho thấy điều đó thành
chữ: tab nội dung mượn thẳng `className="method-tabs"`.

Phân biệt bằng **loại thay đổi, không phải bằng cùng một vạch to nhỏ khác nhau**:

| Tầng | Là gì | Đánh dấu bằng |
| --- | --- | --- |
| Tab cửa sổ, tab request REST | Tài liệu đang mở | **Hình khối**: tab là một thẻ liền mặt với pane bên dưới. Bỏ vạch 2px — thẻ liền mặt đã là tín hiệu, vạch chỉ là nói lại |
| Data / Structure / Statistics / Query | Điều hướng giữa các view | **Gạch chân 2px + weight 600.** Đây là chỗ duy nhất giữ vạch, nên nó lấy lại được nghĩa |
| Loại DB, TCP/IP\|SSH | Lựa chọn về form | **Segmented control**: ô được chọn tô nền, không gạch chân. Tách khỏi `.method-tabs`, dùng lại control mà pane Appearance đã có (Light/Dark/System) |
| Tools encode | Lựa chọn về panel | Cùng segmented control ở trên; bỏ cách viết thứ ba |

Việc này đụng `TabStrip.module.css`, `db.css` và `encode/Panel.module.css`, và cần tách
`.method-tabs` thành hai thứ. `--tab-accent` — knob công khai mà `db.css` dùng để tô vạch amber cho
connection read-only ([`db.css:30`](../../apps/desktop/src/modules/db/db.css#L30)) — **phải sống sót**: khi
tab cửa sổ bỏ vạch, dấu read-only cần một chỗ khác, không được im lặng biến mất. Xem Rủi ro.

## Rủi ro

- **`em` chồng nhau (cao).** 179 khai báo `em` nghĩa là đổi gốc font-size không cho kết quả đều.
  Một `0.9em` nằm trong một `0.9em` khác sẽ ra 10.5px, đọc không nổi. Giảm nhẹ: rà từng màn hình
  bằng ảnh chụp, và chuyển `em` thành token thang chữ ở chỗ nào phát hiện lệch.
- **Scroll drift ở lưới (cao, nhưng khoanh được).** Nếu `ROW_HEIGHT` trong TS và `--row-h` trong CSS
  lệch nhau dù một pixel, đáy trang trôi khi cuộn tới. Giảm nhẹ: một test đo chiều cao dòng thật và
  so với hằng, cho cả bốn lưới.
- **Font hệ thống khác nhau giữa Windows và macOS (trung bình).** Fira Code có metric cố định; sans
  hệ thống thì không. Một nhãn vừa khít trên Windows có thể tràn trên macOS. Giảm nhẹ: không dựa vào
  bề rộng chữ ở bất kỳ đâu; chỗ nào đang dựa thì đặt `min-width` rõ ràng.
- **Chữ 13px quá nhỏ với một số người (trung bình).** Không có tuỳ chọn cỡ chữ toàn app. Ghi nhận,
  không xử lý trong đợt này — nếu thành vấn đề thật thì đó là một tính năng riêng, không phải một
  con số khác.
- **Dấu read-only mất chỗ đứng (cao, dễ bỏ sót).** `db.css` tô vạch accent của tab thành amber để
  nói "kết nối này không ghi được" ([`db.css:30`](../../apps/desktop/src/modules/db/db.css#L30)). Bỏ vạch ở
  tab cửa sổ mà quên chuyện này là **xoá một cảnh báo về an toàn dữ liệu bằng một thay đổi thẩm mỹ**
  — và nó sẽ biến mất im lặng, không test nào đỏ. Giảm nhẹ: dấu read-only phải có chỗ mới **trước
  khi** vạch bị bỏ, không phải sau; và tab read-only vẫn còn badge chữ, nên chỗ mới có thể là badge
  đó được tô đậm lên. Kiểm bằng mắt trên một connection đánh dấu read-only.
- **Tab Terminal có cỡ chữ riêng (thấp).** Pane Terminal trong Settings đã cho chỉnh font và cỡ
  riêng; đợt này không được ghi đè lên nó.

## Kiểm chứng

- `npm run lint` và `npm test` sạch.
- Test mới: **không rule nào cho ô của lưới padding dọc.** Vitest chạy ở môi trường node, không có
  DOM, nên không đo được chiều cao dòng thật. Nhưng CSS lấy chiều cao từ `--row-h`, vốn đến từ
  `ROW_HEIGHT` — hai bên chỉ lệch được khi có rule cộng thêm chiều cao mà `height` không nuốt, tức
  padding hoặc border dọc. Đó là thứ khẳng định được từ stylesheet, và là đúng cái đã hỏng một lần.
  Phần còn lại — đáy trang có trôi khi cuộn không — kiểm bằng tay, và spec ghi nhận đó là chỗ test
  không với tới.
- Test mới: `--font-ui` và `--font-mono` đều được định nghĩa trên `:root` — token chưa định nghĩa
  chính là lỗi mà spec này tìm ra, và nó im lặng.
- Chụp ảnh đối chiếu từng màn: Connection, Data grid, Structure, Statistics, Query, REST, Terminal,
  Tools, Settings — ở cả light và dark.
- Mở một connection đánh dấu **read-only** và xác nhận dấu đó vẫn thấy được sau phần 6.
- Mở bốn tầng cùng lúc (tab cửa sổ → workspace tab → picker trong form) và xác nhận ba tầng trông
  khác nhau chứ không phải cùng một vạch ba lần.
- Đọc lại đoạn văn Liquid glass trong Settings. Nếu nó vẫn khó đọc thì phần 1 chưa xong.
