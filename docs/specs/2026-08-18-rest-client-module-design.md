---
status: implemented
date: 2026-08-18
---

# Module REST client

Ngày: 2026-08-18

## Mục tiêu

The standalone client là một shell cộng một module. Hôm nay chỉ có `db`. Spec này mô tả module thứ hai: một REST
client — soạn request HTTP, gửi, xem response — sống cạnh `db` mà không ai trong hai bên biết
khái niệm của bên kia.

Sau khi làm xong:

- Mở tab REST từ menu `[+]`, dán một lệnh cURL, bấm gửi, đọc response ở dạng render / cây / thô.
- Request lưu lại giữa các lần mở app; request sinh ra do dán thì tự đến tự đi.
- Biến `{{var}}` theo environment, đổi environment ngay trong giao diện request.
- Không file nào ngoài `src/modules/rest/` biết khái niệm nào của HTTP, trừ đúng hai dòng mà
  [adding-a-module](../standards/desktop/adding-a-module.md) cho phép.
- Toàn bộ phần dễ sai — parse cURL, nội suy biến, nhận diện kiểu nội dung — là hàm thuần và nằm
  trong `npm test`.

## Phi mục tiêu

Ghi ra để không bị kéo vào:

- **Không streaming.** SSE, `text/event-stream`, thanh tiến trình khi tải nặng: response là một giá
  trị, không phải một dòng chảy. Nút Cancel thì có ngay từ đầu.
- **Không WebSocket, không GraphQL, không gRPC.**
- **Không collection dạng cây.** Sidebar là hai nhóm phẳng, không folder, không kéo thả.
- **Không import/export** Postman, Insomnia, OpenAPI.
- **Không script trước/sau request**, không test assertion, không chuỗi request nối nhau.
- **Không cookie jar.** Cookie muốn gửi thì tự đặt header.
- **Không lưu response thành file.** Endpoint trả PDF thì xem được kiểu và kích thước, không mở ra
  được. Cắt vì phạm vi, không vì khó — xem mục 4.
- **Không tách DNS / TCP / TLS timing.** Chỉ tổng thời gian và thời điểm nhận xong header.
- **Không session restore.** Các tab request đang mở mất khi thoát app, giống mọi tab khác của
  the standalone client. Thứ đáng giữ được giữ theo cách khác — xem mục 2.
- **Không đụng ba splitter của module db.** Viết `Splitter` dùng chung và dùng nó ở module rest;
  chuyển ba chỗ kia sang là một commit riêng, quyết định sau.
- **Không thêm jsdom hay test component.** Repo cố ý chỉ test logic thuần.

## Hiện trạng

Những gì đã có sẵn và spec này dựa vào:

| Chỗ | Dùng để làm gì |
| --- | --- |
| `src/shell/module.ts` | `ModuleDefinition` — id, Icon, Tab, `settings`, `shortcuts` |
| `src/shell/registry.ts` | Một dòng nữa trong `MODULES` |
| `src/i18n/dicts.ts` | Gộp từ điển của module, gồm nhóm `error` gộp tay |
| `src-tauri/src/secrets.rs` | `secrets_save`/`secrets_load`, id là chuỗi bất kỳ — dùng lại nguyên xi |
| `src/modules/db/savedConnectionsStore.ts` | Pattern store dùng chung giữa mọi tab |
| `src/modules/db/queryDrafts.ts` | Pattern bản nháp bám theo dữ liệu, không bám theo tab |
| `src/modules/db/components/QueryEditor/QueryHistoryDialog.tsx` | Pattern dialog lịch sử |
| `src/components/` | `Button`, `Input`, `Select`, `ItemList`, `ErrorBanner`, `ConfirmDialog`, `Tooltip`, `JsonView` |
| `src/core/shortcuts/` | `decide()` — chỉ xử lý chord có trong catalogue |
| `@codemirror/*` | Đã cài, có `SqlEditor` làm mẫu |

Hai điều kiểm tra được trong code, quyết định thiết kế bên dưới:

1. **Shell không lưu tab nào.** [`App.tsx:39`](../../apps/desktop/src/shell/App.tsx#L39) là
   `useState<TabInfo[]>([newTab()])`. Không có store, không có khôi phục.
2. **`Ctrl+V` không bị dispatcher nuốt.** `decide()` chỉ hành động với chord nằm trong catalogue và
   trả `{do:"nothing"}` cho phần còn lại; ngoài ra `isTextEntry` bật `press.typing` trong `<input>`
   nên def có `whenTyping: "ignore"` bị lọc ra. Bắt sự kiện `paste` của DOM là đủ và đúng chỗ.

Backend chưa có HTTP client nào — `Cargo.toml` không có `reqwest` lẫn `tauri-plugin-http`.

## Quyết định nền: Rust chỉ là đường ống

Rust có đúng hai command. `rest_send` nhận một request **đã giải xong** — URL cuối, header cuối,
body ở dạng đã encode — dựng `reqwest::Request`, bắn, đo, trả bytes. `rest_cancel` cắt nó giữa
chừng.

Mọi thứ còn lại là TypeScript: nội suy `{{var}}`, parse cURL, ghép query, đoán kiểu nội dung, chuỗi
fallback của viewer. Ba lý do:

- Đó là nơi bug thật sự nằm, và ở TypeScript thì `npm test` phủ được mà không cần chạy app —
  `cargo test` chưa phải văn hoá của repo này.
- Vì frontend tự nội suy, thanh URL hiện được **URL thật sau khi thay biến** trước lúc bấm gửi.
  Đẩy việc đó xuống Rust là mất tính năng đó.
- `reqwest` mặc định dùng `native-tls`, trùng đúng backend TLS mà `sqlx` đang dùng — không kéo
  thêm một stack TLS thứ hai vào bundle.

Streaming qua Tauri event đổi hình dạng response từ một giá trị thành một dòng chảy và làm phức
tạp toàn bộ tầng trên; để lại v2. Nhưng `rest_send` mang sẵn `request_id` ngay từ đầu, nên thêm
được mà không phá hợp đồng.

## 1. Kiến trúc và ranh giới

### Frontend

```
src/modules/rest/
  index.ts              ModuleDefinition
  RestTab.tsx           sidebar | splitter | main
  types.ts  api.ts  rest.css  shortcuts.ts  i18n/{en,vi}.ts

  -- logic thuần, có test --
  interpolate.ts        {{var}} -> giá trị
  parsePaste.ts         cURL / URL -> RestRequest, và toCurl ngược lại
  syncUrlParams.ts      ô URL <-> bảng Params
  buildRequest.ts       state UI -> WireRequest
  contentType.ts        nhận diện kiểu + availableModes()

  -- state chia sẻ giữa mọi tab REST --
  requests.ts requestsStore.ts
  environments.ts environmentsStore.ts
  history.ts

  components/  RequestList, RequestTabs, UrlBar, KeyValueTable, BodyEditor,
               AuthPane, ResponseStatusBar, ResponsePane, HtmlPreview,
               TreeView, HexView, EnvironmentDialog, HistoryDialog, RestSettings
```

### Backend

```
src-tauri/src/modules/rest/
  mod.rs        register() - builder.manage(RestState::default())
  commands.rs   rest_send, rest_cancel
  models.rs     WireRequest, WireBody, RestResponse
  state.rs      reqwest::Client dùng lại + Mutex<HashMap<String, CancellationToken>>
```

Cộng một dòng `reqwest` ở `Cargo.toml`, một dòng ở `lib.rs`, một block ở `modules::handler()`.

### Dùng chung

`src/components/Splitter/` — cần hai splitter (sidebar↔main, request↔response). Repo hiện có ba
bản chép tay gần giống nhau trong `SqlWorkspace` / `MongoWorkspace` / `RedisWorkspace`; **không**
đụng vào chúng trong phạm vi này.

### Hai thứ module này là người đầu tiên chạy thật

Theo đúng cảnh báo cuối [adding-a-module](../standards/desktop/adding-a-module.md): nhánh
menu của nút `[+]` (viết rồi nhưng chưa từng chạy vì mới có một module), và `registry.ts` lần đầu
chứa hai module. Cả hai phải kiểm bằng tay.

## 2. Data model và persistence

### Request

```ts
type Method = "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS";

interface KeyValue { id: string; enabled: boolean; key: string; value: string }

type Body =
  | { kind: "none" }
  | { kind: "raw"; language: "json" | "xml" | "html" | "text"; text: string }
  | { kind: "form"; fields: KeyValue[] }
  | { kind: "multipart"; fields: (KeyValue & { file?: string })[] }
  | { kind: "binary"; filePath: string };

type Auth =
  | { kind: "none" }
  | { kind: "bearer"; token: string }
  | { kind: "basic"; username: string; password: string }
  | { kind: "apiKey"; name: string; value: string; in: "header" | "query" };

interface RestRequest {
  id: string;
  name: string;                    // rỗng thì sidebar hiện URL rút gọn
  method: Method;
  url: string;                     // giữ nguyên {{var}}
  params: KeyValue[];
  headers: KeyValue[];
  body: Body;
  auth: Auth;
  origin: "manual" | "paste";
  createdAt: number;
  lastUsedAt: number;
}
```

**Query sống ở tab Params, ô URL chỉ giữ phần gốc.** URL gửi đi là ô URL ghép với các dòng Params
đã tick; dòng bỏ tick vẫn nằm trong bảng — đó là cách duy nhất tạm tắt một param mà không mất nó. Hai
bên không còn đồng bộ hai chiều: sửa Params không viết lại ô URL, gõ `?page=2` vào ô URL cũng chưa
đụng tới bảng.

Query trong ô URL xuống Params ở hai lúc, và ô URL còn lại phần gốc và `#hash`:

- **Khi dán** một URL: query dán vào **thay** các dòng đã tick, dòng bỏ tick giữ nguyên chỗ cũ. Dán
  một URL không có query thì bảng Params để nguyên. Một lệnh cURL là cả một request, nên query của
  nó thành Params của request mới ấy.
- **Khi Send**: query gõ tay được **gom** vào bảng — mỗi cặp khớp một-một với một dòng đã tick cùng
  key và value thì coi như đã có, cặp còn lại thêm vào cuối bảng, đã tick. Luật khớp là cho request
  lưu từ trước thay đổi này, khi URL và Params còn là hai bản sao của nhau — gom chúng không sinh
  dòng trùng. Chưa gom thì dòng preview và phần kiểm tra biến vẫn tính trên bản đã gom, nên luôn nói đúng
URL sẽ đi. Logic ở `syncUrlParams.ts`.

### Bốn file trên đĩa

| File | Chứa gì |
| --- | --- |
| `rest-requests.json` | `{ saved: RestRequest[], recent: RestRequest[] }`, Recent cắt còn 10 |
| `rest-environments.json` | env + **tên** biến + cờ `secret`; giá trị secret không ở đây |
| `rest-history.json` | lịch sử gửi, cắt còn 100 |
| `rest-workspace.json` | `lastEnvId`, `sidebarWidth`, `splitRatio`, và các thiết lập ở mục 6 |

Cả bốn đọc một lần, ghi qua store dùng chung, mọi tab REST thấy cùng một thứ — pattern của
`savedConnectionsStore.ts`.

### Biến secret đi vào OS credential store

`secrets_save(id, secrets)` / `secrets_load(id)` nhận `id` là chuỗi bất kỳ và `secrets` là
`HashMap<String, String>`. Dùng lại nguyên xi, **không viết gì mới**: khoá `rest-env:<envId>`, giá
trị là map `tên biến -> giá trị`.

Nhờ vậy `rest-environments.json` đọc được và copy được — biết env Dev có biến gì, `baseUrl` trỏ đâu
— mà token thì nằm trong Windows Credential Manager cạnh mật khẩu MySQL, đúng đường lối
[`secrets.rs`](../../apps/desktop/src-tauri/src/secrets.rs) tự viết ra cho mình.

### Cái gì sống qua restart

Shell không lưu tab nào, nên "khôi phục các tab request đang mở" không có nền để đứng — và nếu mở
hai tab REST cùng lúc thì hai bên sẽ ghi đè trạng thái của nhau.

Thay vào đó, theo cách `queryDrafts.ts` làm: **bản nháp bám theo request, không bám theo tab.** Sửa
dở một request rồi đóng tab, mở lại từ sidebar thì thấy nguyên chỗ đang sửa, vì phần sửa đã ghi
thẳng vào chính request đó. Danh sách tab đang mở chỉ nằm trong bộ nhớ.

Hệ quả: **không có trạng thái "chưa lưu"**, không có nút Save, không bao giờ có hộp thoại "bạn có
muốn lưu không". Ít trạng thái thì ít bug.

### Environment

```ts
interface EnvVar { name: string; value: string; secret: boolean }
interface Environment { id: string; name: string; vars: EnvVar[] }
```

Một danh sách environment duy nhất, dùng chung toàn app. **Danh sách request không phụ thuộc
environment** — đổi env không đổi danh sách, nó chỉ đổi giá trị mà `{{var}}` giải ra lúc gửi.

Dropdown chọn env ghim ở **cuối tab strip, mép phải** — nằm trong giao diện làm việc với request,
luôn thấy khi đang sửa, và ở tầm đúng: env là thuộc tính của cả workspace chứ không của một request.
Sửa danh sách qua mục *Manage environments…* cuối dropdown, mở một modal: trái là danh sách env,
phải là bảng biến (tên / giá trị / cờ secret). Giá trị secret hiện dấu chấm, có nút mở mắt.

Env đang chọn là state của từng tab REST (mở hai tab để so dev với prod), và `lastEnvId` lưu vào
`rest-workspace.json` để tab REST mới mở bắt đầu từ đó.

### Lịch sử

```ts
interface HistoryEntry {
  id: string;
  requestId: string | null;        // null nếu request đã bị xoá
  envName: string;                 // chụp lại tên, đọc sau vẫn hiểu
  method: Method; url: string;     // đã nội suy, TRỪ biến secret
  startedAt: number; durationMs: number;
  status: number | null; statusText: string;
  size: number;
  error: string | null;
  responseBody: string | null;     // base64, tối đa 256 KB, null khi công tắc tắt
}
```

URL và header lưu ở dạng **đã nội suy mọi biến trừ biến đánh dấu secret** — `Bearer {{token}}` giữ
nguyên chữ. Đọc lại vẫn thấy host thật, path thật; chỉ token là không.

Response body thì không có cách nào biết chỗ nào là bí mật, nên có công tắc *Lưu nội dung response
vào lịch sử* ở pane Settings (mục 6), bật mặc định. **Tắt nó thì xoá luôn body của các entry đã
lưu**, không chỉ ngừng ghi từ đó — một công tắc riêng tư mà để lại dữ liệu cũ nằm nguyên trên đĩa là
một lời nói dối.

Hiển thị bằng dialog mở từ header sidebar, pattern `QueryHistoryDialog.tsx`.

## 3. Luồng gửi

### Luật nội suy (`interpolate.ts`)

Biến là `{{` + tên khớp `[A-Za-z0-9_.-]+` + `}}`. Phần trong ngoặc không khớp charset đó —
`{{#each items}}`, `{{ x }}` có khoảng trắng — **không phải biến**, để nguyên; nhờ vậy body chứa
template Handlebars gửi lên server đi qua sạch sẽ. Ép literal bằng `\{{name}}`.

Áp vào: URL, giá trị params, cả key lẫn value của headers, body raw, giá trị form/multipart, và các
field của Auth. **Không** áp vào đường dẫn file — đó là đường dẫn thật trên máy.

Lồng nhau được (`baseUrl = https://{{host}}`), lặp tối đa 5 vòng rồi báo lỗi vòng lặp.

**Thiếu biến thì chặn gửi.** Nút Send tắt, báo rõ *thiếu `token` trong environment `Dev`*, kèm nút
thêm biến đó vào env đang chọn. Gửi một request có chữ `{{token}}` trong header Authorization không
giúp được ai. Dòng bỏ tick thì biến trong đó không tính — đó là cách tạm tắt một dòng đang hỏng.

Dropdown env có mục **None**, và nó là mặc định khi chưa ai tạo environment nào. Chọn None thì
`interpolate` không chạy: `{{var}}` ra dây nguyên chữ và Send **không** bị chặn. Luật chặn chỉ có
nghĩa khi đã chọn một env cụ thể — lúc đó thiếu biến là một lỗi thật, còn ở None thì `{{` chỉ là ký
tự người dùng gõ vào.

Dưới ô URL có một dòng preview URL sau nội suy, tên biến thiếu tô đỏ. Dòng này luôn hiện, kể cả ở
None: ô URL không mang query, nên đây là chỗ duy nhất thấy trọn URL sẽ gửi. Mặc định một dòng, phần
còn lại sau nút mở rộng.

### Hợp đồng với Rust

```rust
async fn rest_send(state, req: WireRequest) -> Result<RestResponse, AppError>
async fn rest_cancel(state, request_id: String) -> Result<(), AppError>
```

`WireRequest`: `request_id`, `method`, `url` (đã giải), `headers: Vec<(String, String)>`, `body`,
`timeout_ms`, `follow_redirects`, `accept_invalid_certs`.

`WireBody` một trong bốn: `none`; `text` (raw và form-urlencoded — frontend tự encode và tự đặt
Content-Type); `file { path }` (binary); `multipart { parts }` (part file chỉ mang đường dẫn).

Multipart là thứ duy nhất Rust tự dựng, vì boundary và stream file từ đĩa là việc của `reqwest`. Đó
vẫn là vận chuyển, không phải logic.

`RestState` giữ một `reqwest::Client` dùng lại — giữ connection pool và TLS session, nên request thứ
hai tới cùng host nhanh hơn hẳn — và `Mutex<HashMap<String, CancellationToken>>`. `rest_send` chạy
trong `tokio::select!` với token của chính nó. Nút Send đổi thành Cancel trong lúc chạy.

```rust
struct RestResponse {
  status: u16, status_text: String, http_version: String,
  headers: Vec<(String, String)>,   // Vec chứ không phải map: Set-Cookie lặp nhiều lần
  body_base64: String,
  body_size: u64,                   // bytes thật, kể cả phần đã cắt
  truncated: bool,
  final_url: String,
  total_ms: u64, ttfb_ms: u64,
}
```

- **Body luôn về dạng base64**, kể cả text — response có thể là ảnh, PDF, gzip; không thể giả định
  UTF-8 ở tầng Rust. Frontend decode ra bytes rồi mới quyết định đọc thành chữ hay không. Giá phải
  trả là phình 33% qua IPC, chấp nhận được ở mức vài MB.
- **Cắt ở 16 MB**, `truncated: true`. Lớn hơn thế mà nhồi qua IPC rồi giữ trong bộ nhớ webview là
  đường dẫn tới treo app.
- **Chỉ `total_ms` và `ttfb_ms`.** Tách DNS / TCP / TLS thì `reqwest` không đưa ra nếu không tự cài
  hook ở tầng connector; không hứa thứ chưa chắc làm được.

## 4. Response viewer

### Thanh trạng thái

Cao đúng bằng hàng method/URL/Send bên trái:

```
● 500 Internal Server Error      142 ms      1.2 KB      -> 2 redirect
```

Màu theo lớp mã: 2xx xanh, 3xx lam, 4xx cam, 5xx đỏ. `142 ms` có tooltip ghi rõ đó là **tổng** thời
gian. Kích thước là của body; bị cắt thì tooltip ghi kích thước thật. Chỉ báo redirect chỉ hiện khi
có redirect, tooltip là URL cuối cùng.

### Bốn tab

`Preview` · `Source` · `Raw` · `Headers (14)`.

Tab `Raw` có nút copy cạnh *Wrap lines*: copy trọn body dạng chữ, kể cả phần bị cắt khi hiển thị.
Body nhị phân hiện dạng hex thì không có nút này.

Tab `Headers` không có trong mô tả gốc nhưng nửa số lần người ta mở REST client là để xem
`Set-Cookie`, `Location`, `X-RateLimit-Remaining`. Bảng hai cột, giữ nguyên thứ tự và giữ cả header
trùng tên — đó là lý do Rust trả `Vec`.

### Nhận diện kiểu (`contentType.ts`)

Header `Content-Type` được tin trước. Chỉ khi nó vắng mặt hoặc chung chung
(`application/octet-stream`, `text/plain` mà thân lại là JSON) mới ngửi bytes: `{`/`[` mà parse được
-> json; `<!doctype html` -> html; `<?xml` -> xml; magic bytes PNG/JPEG/GIF/WEBP -> image; `%PDF-`
-> pdf. Charset lấy từ `Content-Type`, mặc định UTF-8; bytes không hợp lệ với charset đó -> nhị phân.

| Kiểu | Preview | Source | Raw |
| --- | --- | --- | --- |
| json | in đẹp, tô màu | cây gập được | chuỗi thô |
| html | iframe sandbox | cây DOM | chuỗi thô |
| xml | — | cây | chuỗi thô |
| text | — | — | chuỗi thô |
| image | ảnh | — | hex dump |
| pdf / nhị phân | thẻ mô tả kiểu + kích thước | — | hex dump |

XML là ví dụ đúng cho luật fallback: không có gì để render đẹp nên Preview tắt, mặc định tụt xuống
Source. Nhị phân tụt tiếp xuống Raw. `availableModes(kind)` là hàm thuần, có test — chuỗi fallback
là logic, không phải mấy dòng `if` rải trong JSX.

**Lựa chọn của người dùng được nhớ theo tab request, và nhớ đúng cách.** Chọn Source rồi gửi một
request trả về ảnh: hiện Preview vì Source không dùng được, nhưng *ghi nhớ* vẫn là Source — gửi lại
một response JSON thì tự về Source. Không có kiểu tụt xuống rồi ở lì dưới đó.

### Preview HTML

`<iframe sandbox srcdoc={html}>` với `sandbox` **rỗng** — mức khoá chặt nhất, không `allow-scripts`,
không `allow-same-origin`. Script của server không chạy và không có đường nào chạm tới IPC của
Tauri.

Không chèn `<base href>` mặc định, nên ảnh và CSS ngoài không tải — trang hiện ra là bố cục với CSS
nội tuyến. Có checkbox *Tải tài nguyên ngoài* ngay đầu preview, tắt sẵn; bật thì chèn
`<base href={finalUrl}>` và trang nhìn đúng như thật. Để tắt mặc định vì bật lên là app tự đi gọi
tới host đó — pixel theo dõi trong một response HTML sẽ nổ.

### Cây Source

`TreeView` viết mới trong `modules/rest/components/`. `DocumentNode.tsx` của Mongo làm việc tương tự
nhưng nằm trong module db và **không được import qua ranh giới module**; nó cũng là cây sửa được
còn cái này chỉ đọc, nên phần lớn code kia không dùng lại được.

JSON dựng cây từ `JSON.parse`. HTML và XML dùng `DOMParser` có sẵn trong webview — parse `text/html`
không chạy script và không tải tài nguyên nào. XML parse hỏng -> Source tắt, tụt xuống Raw.

Mỗi node gập/mở được, chuột phải copy giá trị hoặc copy đường dẫn (`$.data.items[3].id`). Tìm kiếm
trong cây để v2.

**Giới hạn thật thà**: body trên 2 MB thì tab Source tắt kèm dòng giải thích — cây chưa ảo hoá và
2 MB JSON là hàng trăm nghìn node, dựng hết ra DOM sẽ treo. Raw vẫn xem được.

### Raw

`<pre>` giữ nguyên từng ký tự server trả về, có nút bật/tắt xuống dòng. Trên 5 MB thì chỉ hiện 5 MB
đầu kèm dòng báo. Nhị phân thì Raw là hex dump ba cột (offset · bytes · ASCII), cũng cắt ở 5 MB.

## 5. Sidebar, paste, tab strip

### Sidebar

Header: `[+ New request]`, nút mở dialog lịch sử, ô lọc theo tên/URL. Dưới là hai nhóm gập được.

**SAVED** — không giới hạn. `[+ New request]` vào thẳng đây với tên `New request`.

**RECENT (n/10)** — chỉ chứa request sinh do dán. Đầy thì rớt cái **lâu nhất không dùng** theo
`lastUsedAt`, không phải cái tạo sớm nhất: một request vẫn được gửi mỗi ngày không có lý do gì bị
đẩy ra.

Mỗi dòng có badge method tô màu. Chuột phải: Đổi tên, Nhân bản, **Sao chép dạng cURL**, Xoá. Dòng
Recent có thêm nút ghim — ghim là chuyển hẳn sang Saved và đổi `origin` thành `manual`. Xoá ở Saved
hỏi lại qua `ConfirmDialog`; ở Recent không hỏi, vì nó vốn tự đến tự đi.

Hai điểm dễ hiểu nhầm, chốt rõ: **sửa một request Recent không đẩy nó sang Saved** — ghim là hành
động duy nhất làm việc đó, nên một request đang sửa dở vẫn có thể bị rớt khi Recent đầy. Và
`lastUsedAt` cập nhật **khi bấm gửi**, không phải khi mở tab, nên mở ra xem rồi đóng lại không cứu
một request khỏi bị rớt.

`toCurl` là hàm ngược của bộ parse, viết cùng file và test cùng nhau.

### Paste thông minh

Trên sự kiện `paste` của ô URL, đọc `clipboardData`, đưa qua `parsePaste(text)`:

1. **cURL** — text sau khi trim bắt đầu bằng `curl`. Nối dòng bị ngắt bằng `\` hoặc `^`, tách token
   theo luật nháy của shell. Hiểu `-X/--request`, `-H/--header`,
   `-d/--data/--data-raw/--data-binary`, `-F/--form`, `-u/--user`, `--url`, `-G`; bỏ qua `-L`, `-k`,
   `--compressed` vì ba thứ đó là thiết lập toàn cục ở pane Settings chứ không thuộc về một request.
2. **URL** — `new URL(text)` chạy được và giao thức là http/https. Query tách thành các dòng Params,
   phần còn lại vào ô URL. Một lệnh cURL cũng vậy: query của nó xuống Params, không ở lại ô URL.
3. **Không khớp** — không gọi `preventDefault()`, webview dán nguyên văn.

Một chỗ curl và thực tế lệch nhau: `curl -d` với một chuỗi JSON mà không kèm header thì **đúng chuẩn
curl** là `application/x-www-form-urlencoded`, nhưng người ta dán vào đây gần như luôn là JSON. Quy
tắc: body parse được thành JSON hợp lệ -> Body = raw/json; không thì form-urlencoded.

**Dán vào đâu**: tab hiện tại còn trống (chưa gõ gì, chưa gửi lần nào) thì điền tại chỗ; tab đã có
nội dung thì mở tab mới. Nuốt mất thứ đang soạn là mất dữ liệu; mở tab mới thì không phá gì nên
cũng chẳng cần undo.

Dán trùng một request đã có trong Recent (cùng method + URL) thì không đẻ bản thứ hai — đẩy cái cũ
lên đầu và cập nhật `lastUsedAt`.

### Tab strip

Bấm một dòng ở sidebar: đã mở thì nhảy tới tab đó, chưa mở thì mở tab mới. Đóng bằng nút đóng hoặc
chuột giữa, không hỏi gì vì không có gì chưa lưu. Tràn thì cuộn ngang, dropdown env vẫn ghim cứng
bên phải. Chưa mở request nào thì main là màn hình trống với hai gợi ý: dán cURL vào đây, hoặc bấm
`[+]`.

### Phím tắt

Qua `ModuleDefinition.shortcuts`, đúng cách `DB_SHORTCUTS` đang làm. Nhãn lấy từ từ điển của chính
module rest, không đụng nhóm `shortcuts.*` mà shell sở hữu.

| Chord | Việc |
| --- | --- |
| `Ctrl/Cmd + Enter` | Gửi request đang mở, chạy cả khi con trỏ đang trong body |
| `Ctrl/Cmd + N` | Request mới |
| `Ctrl/Cmd + W` | Đóng tab request đang mở |
| `Ctrl/Cmd + H` | Mở lịch sử |

## 6. Lỗi và pane Settings

### Mã lỗi

`reqwest::Error` không tách được DNS, TCP và TLS một cách đáng tin — `is_connect()` gộp cả ba. Tách
ra thì phải dò chuỗi `source()` và so khớp văn bản của thư viện bên dưới, thứ sẽ hỏng lặng lẽ ở lần
nâng cấp kế tiếp. Nên:

| Mã | Khi nào |
| --- | --- |
| `error.restTimeout` | `is_timeout()` |
| `error.restConnect` | `is_connect()` — kèm nguyên văn message gốc, nơi chữ "dns error" hay "certificate verify failed" thực sự nằm |
| `error.restRedirectLoop` | `is_redirect()` |
| `error.restInvalidUrl` | URL không dựng được `reqwest::Url` |
| `error.restFileUnreadable` | file của multipart hoặc binary không đọc được |
| `error.restBuildFailed` | dựng multipart hỏng |

Thêm vào nhóm `error` gộp tay trong `src/i18n/dicts.ts`.

### Lỗi mạng khác với response lỗi

- **`500 Internal Server Error` là một lần gửi thành công.** Về pane response như mọi response khác,
  thanh trạng thái đỏ, body đầy đủ. Không có banner nào.
- **Timeout, không nối được, TLS từ chối** — không có response nào tồn tại. `AppError` hiện trên
  `ErrorBanner` đặt ở đầu pane response, ngay cạnh thứ vừa hỏng; pane giữ nguyên kết quả lần trước.
- **Huỷ không phải lỗi.** Không banner; thanh trạng thái ghi "Đã huỷ".

### Pane Settings của module

Qua `ModuleDefinition.settings` — cơ chế shell đã có sẵn:

- *Lưu nội dung response vào lịch sử* — bật mặc định; tắt thì xoá body đã lưu
- *Xoá toàn bộ lịch sử*
- *Timeout* — mặc định 30 giây
- *Đi theo redirect* — bật mặc định
- *Chấp nhận chứng chỉ tự ký* — tắt mặc định

Ba thứ cuối là toàn cục, không làm per-request ở v1.

## 7. Kiểm thử

`npm test` phủ toàn bộ phần logic thuần:

- `interpolate` — biến lồng nhau, vòng lặp, escape literal, `{{#each}}` để nguyên, thiếu biến trong
  dòng đã bỏ tick thì không tính
- `parsePaste` + `toCurl` — **khứ hồi**: dán một lệnh curl vào rồi copy ra phải cho lại đúng lệnh đó
- `syncUrlParams` — dán thay dòng đã tick, gom khi Send không sinh dòng trùng, dòng bỏ tick không lọt vào URL
- `contentType` + `availableModes` — chuỗi fallback, ngửi bytes khi header chung chung, charset hỏng
- `buildRequest` — state UI ra `WireRequest`, gồm Content-Type tự đặt
- luật Recent — cắt 10 theo `lastUsedAt`, gộp trùng, ghim chuyển nhóm

**Không test nào nói được**, phải mở `npm run dev:app` bấm tay: nhánh menu `[+]`, `registry.ts` với
hai module, iframe sandbox có thật sự chặn script, keyring ghi/đọc biến secret, upload multipart,
nút Cancel cắt giữa chừng. Endpoint để thử: `httpbin.org`, cộng một endpoint trả ảnh và một trả PDF.

Phía Rust không có test — repo chưa có văn hoá `cargo test` và spec này không mở ra. Bù lại lớp Rust
mỏng đúng bằng "dựng request, bắn, đo".

Sau mỗi phase: `npm run build`, `npm test`, và hai lệnh grep ranh giới trong
[adding-a-module](../standards/desktop/adding-a-module.md).

## 8. Phase

| Phase | Nội dung | Xong thì dùng được |
| --- | --- | --- |
| **1** | Khung module + registry + `Splitter`, sidebar hai nhóm, tab strip, UrlBar, Params/Headers, Body raw, `rest_send`/`rest_cancel`, thanh trạng thái, bốn tab response + luật fallback | **Có** |
| **2** | Paste cURL/URL, Copy as cURL, luật Recent | Có |
| **3** | Tab Auth, body form-urlencoded / multipart / binary | Có |
| **4** | Environment: dropdown, dialog, nội suy, biến secret qua keyring | Có |
| **5** | Lịch sử + pane Settings | Có |

Phase 1 tự nó là một REST client chạy được. Dừng ở bất kỳ vạch nào cũng có sản phẩm, không phase nào
để lại thứ dở dang.

Hai thứ Phase 1 cần nhưng chưa có nguồn, nên cắm cứng cho tới đúng phase của chúng:

- **Thiết lập gửi.** `WireRequest` mang `timeout_ms`, `follow_redirects`, `accept_invalid_certs`
  ngay từ Phase 1 — hợp đồng với Rust không đổi về sau. Nhưng pane Settings sinh ra ở Phase 5, nên
  tới lúc đó ba giá trị này cắm cứng ở `30_000`, `true`, `false`. Phase 5 chỉ thay nguồn của chúng.
- **Nội suy.** Environment sinh ra ở Phase 4, nên trước đó `buildRequest` không gọi `interpolate` gì
  cả: `{{var}}` đi thẳng ra dây như chữ thường, không chặn gửi, không có dòng preview URL dưới ô
  nhập. Phase 4 chèn `interpolate` vào giữa và bật hai thứ kia lên cùng lúc.

Mỗi phase kèm một dòng trong `## [Unreleased]` của `CHANGELOG.md` theo
[.agent/conventions/changelog.md](../standards/desktop/changelog.md).
