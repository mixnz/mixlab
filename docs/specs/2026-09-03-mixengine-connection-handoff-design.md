---
status: implemented
date: 2026-09-03
---

# Nhận kết nối từ MixEngine: `<old>://connect` và mật khẩu trong môi trường

Ngày: 2026-09-03

## Mục tiêu

MixEngine (task T83 bên đó) tìm the standalone client đã cài trên máy, **chạy thẳng binary** với một URL làm
`argv[1]` và mật khẩu nằm trong **biến môi trường của đúng tiến trình đó** — không có trong URL,
không có trên dòng lệnh, không có trên đĩa:

```
<old>.exe "<old>://connect?kind=mysql&host=127.0.0.1&port=3306&user=root&database=blog&label=mariadb%40main&password_env=MIXENGINE_DB_PASSWORD"
   với MIXENGINE_DB_PASSWORD=<mật khẩu>   (chỉ trong env của tiến trình này)
```

Spec này là **phía nhận** của hợp đồng đó. Sau khi làm xong:

- `mix database open mariadb@main` trên máy có the standalone client → the standalone client mở lên với một tab tên `mariadb@main`
  **đã nối sẵn** vào server đó, đăng nhập bằng `root`, không phải gõ gì.
- The standalone client **đang mở sẵn** thì lệnh đó mở **thêm một tab** trong cửa sổ đang có và tự nối; tiến trình
  thứ hai thoát mã 0 trong vài chục mili giây (MixEngine đọc đó là `handed_on`).
- Biến môi trường chứa mật khẩu bị **xoá khỏi tiến trình ngay dòng đầu của `run()`**, trước khi
  Tauri sinh thread, trước khi WebView2 fork helper, trước khi module terminal mở shell nào. Shell
  mở trong the standalone client không in ra được nó.
- Mật khẩu không bao giờ vào `connections.json`, `localStorage`, log hay `Debug`. Nó chỉ đi từ env
  → RAM → form → `connect_db`, đúng đường mọi mật khẩu gõ tay đang đi.
- Scheme `<old>://` được đăng ký với hệ điều hành (NSIS, `Info.plist`, `.desktop`), nên một link
  `<old>://connect?...` trong trình duyệt cũng mở được the standalone client — chỉ là **không có mật khẩu**, vì
  không ai đặt được env cho một link.
- Chạy the standalone client lần hai không có URL (bấm icon lần nữa) → cửa sổ đang mở được đưa lên trước, tiến
  trình thứ hai thoát. Đây là hệ quả miễn phí của kênh single-instance.

## Phi mục tiêu

- **Không đọc mật khẩu từ keyring của MixEngine.** Đó là T84 bên MixEngine (một quy ước keyring
  chung). Đợt này mật khẩu đến bằng env, hết.
- **Không thêm kind mới.** URL chỉ nói `mysql` / `postgres` / `redis` — đúng ba từ MixEngine phát
  ra (D5 bên đó; `mariadb` được MixEngine dịch thành `mysql` trước khi gửi). `mongo` không có
  trong hợp đồng, nên không nhận.
- **Không tự lưu kết nối.** Tab mở ra như một kết nối gõ tay: form có sẵn mọi thứ, tên đã điền là
  `label`, người dùng bấm Save nếu muốn. Lưu thì mật khẩu vào kho OS như mọi kết nối khác — đó là
  hành động của người dùng, không phải của handoff.
- **Không dùng `tauri-plugin-single-instance`.** Xem mục 3: nó chỉ chuyển được `argv`, không có chỗ
  cho mật khẩu.
- **Không đăng ký scheme lúc chạy trên Windows/macOS.** Installer làm việc đó. Linux là ngoại lệ có
  lý do (AppImage) — mục 6.
- **Không đổi `ConnectionConfig`.** Năm trường của URL rơi đúng vào năm trường đã có.

## Hiện trạng

| Chỗ | Điều spec dựa vào |
| --- | --- |
| [`src-tauri/src/lib.rs`](../../apps/desktop/src-tauri/src/lib.rs) | `run()` dựng builder ngay dòng đầu; chưa đọc `argv`, chưa có plugin deep-link hay single-instance |
| [`src-tauri/src/secrets.rs`](../../apps/desktop/src-tauri/src/secrets.rs) | `Redacted` — cách mọi `Debug` giấu mật khẩu |
| [`src-tauri/src/modules/db/models.rs`](../../apps/desktop/src-tauri/src/modules/db/models.rs) | `ConnectionConfig` với `Debug` viết tay che `password`/`uri`; `DbKind` serde lowercase |
| [`src-tauri/src/modules/db/commands/mod.rs`](../../apps/desktop/src-tauri/src/modules/db/commands/mod.rs) | `connect_db(config)` → id; mọi thứ sau đó đi theo id |
| [`src/shell/App.tsx`](../../apps/desktop/src/shell/App.tsx) | `openTab(moduleId)`; tab mount với `restored={tab.state}` |
| [`src/shell/module.ts`](../../apps/desktop/src/shell/module.ts) | Khe `restored` / `onStateChange` — thứ duy nhất shell mang cho module, và shell không đọc |
| [`src/modules/db/tabState.ts`](../../apps/desktop/src/modules/db/tabState.ts) | `parseDbTabState` — nơi validate khe đó, chỉ id |
| [`src/modules/db/DbTab.tsx`](../../apps/desktop/src/modules/db/DbTab.tsx) | `connect(config, title, savedId)`, `formFrom(config)`, `restoreTried` |
| `Cargo.lock` | `url 2.5`, `libc`, `tokio` (`full` → có `net`, named pipe trên Windows) đều đã là dep gián tiếp |
| `tauri.conf.json` | `identifier = io.github.haiquang9994.<old>` — tên của mutex/socket/pipe bên dưới |

Ba điều đo được, quyết định thiết kế:

1. **`tauri-plugin-single-instance` gửi nguyên `std::env::args()` rồi `exit(0)` ngay trong `setup`
   của plugin.** Không có hook trước khi gửi, không có chỗ nhét thêm payload. Mật khẩu của tiến
   trình thứ hai không có đường sang tiến trình thứ nhất qua nó.
2. **`tauri-plugin-deep-link` trên Windows/Linux cũng tự đọc `argv` lúc `setup`** và phát
   `deep-link://new-url` nếu `argv[1]` là URL đúng scheme. Trên macOS nó nhận Apple Event
   (`RunEvent::Opened`) sau `setup`. Nghĩa là nếu the standalone client vừa tự đọc `argv` vừa nghe sự kiện của
   plugin trên Windows/Linux thì một lần chạy thành hai tab.
3. **Mật khẩu phải được đọc trước `tauri::Builder`.** Builder sinh thread; WebView2 fork helper kế
   thừa env; `portable-pty` mở shell kế thừa env. Chỉ có `main` đơn luồng ở dòng đầu `run()` là
   chỗ `remove_var` an toàn và kịp.

## 1. Hợp đồng: URL và biến môi trường

Đây là hợp đồng MixEngine ghi trong `features/extensions.md` của nó, chép lại để phía này có cái
mà test:

```
<old>://connect?kind=<mysql|postgres|redis>&host=<h>&port=<p>[&user=<u>][&database=<d>]&label=<l>[&password_env=<NAME>]
```

- `kind`, `host`, `port` bắt buộc. `user`, `database` chỉ có khi có gì để nói (Redis không có
  account → không có `user`, không có `password_env`).
- `label` là service id bên MixEngine (`mariadb@main`) — tên tab và tên điền sẵn vào ô Save.
- `password_env` **tên** biến môi trường chứa mật khẩu. Không phải mật khẩu.
- Giá trị percent-encoded theo RFC 3986 (mọi thứ ngoài unreserved → `%XX`). `@` → `%40`.

**Phía nhận nợ MixEngine ba việc**, nguyên văn hợp đồng: đọc `argv[1]`; đọc biến rồi **xoá nó khỏi
env trước khi bất kỳ thứ gì khác khởi động**; không bao giờ ghi nó vào file kết nối đã lưu. Và một
việc thứ tư cho lúc đã mở sẵn: tiến trình thứ hai **đọc biến trước khi chuyển tiếp**, gửi qua kênh
của chính nó, rồi thoát 0.

### Luật tên biến

`password_env` được tin **có điều kiện**: tên phải khớp `^MIX[A-Z0-9]*_[A-Z0-9_]*PASSWORD$` —
`MIXENGINE_DB_PASSWORD`, `<OLD>_PASSWORD`, `MIXENGINE_REDIS_PASSWORD` đều qua; `PATH`, `HOME`,
`DB_PASSWORD`, `PGPASSWORD` không.

Lý do: một khi scheme được đăng ký với OS (mục 6), **bất kỳ trang web nào** cũng phát được
`<old>://connect?host=attacker&password_env=SOME_VAR`. Windows/Linux khởi động the standalone client với URL đó làm
`argv[1]` — y hệt cách MixEngine gọi — và nếu the standalone client đọc bất kỳ biến nào URL chỉ định thì một cú
click gửi `$SOME_VAR` của người dùng tới `attacker` dưới dạng mật khẩu MySQL. Giới hạn vào
namespace `MIX*_PASSWORD` là chỗ chặn: không ai export một biến tên như thế trong session của mình
ngoài MixEngine, và MixEngine đặt nó **chỉ** cho tiến trình nó khởi động. Tên không khớp → coi như
không có `password_env`, ghi một dòng ra stderr.

Biến bị **xoá dù đọc được hay không**, và dù URL có hợp lệ hay không: một URL hỏng vẫn có thể mang
`password_env` đúng tên, và không có lý do gì để giá trị đó sống tiếp.

## 2. Backend: ba file mới, một lệnh mới

```
src-tauri/src/
  launch.rs              Tiến trình này được mở với cái gì; hàng đợi "mở tab" cho frontend
  instance.rs            Kênh giữa hai tiến trình the standalone client: named pipe / Unix socket
  modules/db/handoff.rs  URL → ConnectionConfig; kho tạm; lệnh handoff_take
```

### `modules/db/handoff.rs` — hiểu URL

```rust
pub struct Handoff { pub config: ConnectionConfig, pub label: String }   // Debug che password

/// Tên biến `password_env` chỉ tới, nếu qua được luật tên. Không đọc env.
pub fn credential_name(url: &str) -> Option<String>;

/// URL + mật khẩu (đã đọc ở nơi khác) → thứ để nối. Thuần, không đụng env, không đụng app.
pub fn parse(url: &str, secret: Option<String>) -> Result<Handoff, AppError>;
```

`parse` dùng `url::Url` (đã có trong cây dep): scheme phải là `<old>`, host phải là `connect`,
`query_pairs()` lo percent-decoding. `kind` ánh xạ thẳng sang `DbKind` (`mysql`, `postgres`,
`redis`); `host` không rỗng; `port` là `u16` khác 0; `user` → `username`, `database` → `database`,
`secret` → `password`; `label` vắng thì `host:port`. `use_ssl` để `None` — "thử TLS, không có thì
plaintext", đúng cho server loopback của MixEngine và không sai cho server xa. `uri`, `ssh` để
`None`. Sai bất kỳ điều gì → `err!("error.handoffInvalid", message = …)`, lời tiếng Anh nói rõ
thiếu gì.

```rust
pub struct HandoffState { pending: Mutex<HashMap<String, Handoff>> }

/// Từ launch.rs: hiểu URL, cất vào kho dưới một uuid, xin shell một tab db trỏ tới uuid đó.
pub fn accept(app: &AppHandle, url: &str, secret: Option<String>) -> Result<(), AppError>;

/// Tab mới gọi đúng một lần: lấy ra và xoá. Id lạ → `error.handoffExpired`.
#[tauri::command]
pub async fn handoff_take(state: State<'_, HandoffState>, id: String) -> Result<Handoff, AppError>;
```

Kho tạm là nơi duy nhất mật khẩu nằm trong RAM phía Rust ngoài `ConnectionConfig` đang bay qua
`connect_db`. Nó được lấy ra **một lần** — tab mở lại từ session cũ với một `handoffId` cũ nhận
`handoffExpired` và im lặng hiện form trống, đúng như một tab mở tay.

`HandoffState` được `modules::db::register` quản lý cùng `DbState`. Lệnh `handoff_take` vào khối
`db` của `modules::handler()`.

### `launch.rs` — tiến trình này được mở với cái gì

```rust
/// argv[1] nếu là `<old>://…`, và mật khẩu lấy ra khỏi env — đọc **một lần, dòng đầu `run()`**.
pub struct Opening { pub url: Option<String>, pub secret: Option<String> }   // Debug che secret

impl Opening {
    pub fn from_process() -> Self;                                    // std::env::args_os + std::env::var/remove_var
    pub fn from_args(args, take_env: impl FnMut(&str) -> Option<String>) -> Self;   // phần thuần, để test
}

/// Một tab shell phải mở, do backend xin. Shell không đọc `state`; module đọc.
#[derive(Serialize)] #[serde(rename_all = "camelCase")]
pub struct TabRequest { pub module_id: &'static str, pub state: serde_json::Value }

pub struct Requests { pending: Mutex<Vec<TabRequest>> }   // managed state
pub fn request(app: &AppHandle, request: TabRequest);      // push + emit "launch://request"

#[tauri::command] pub fn launch_take_requests(state: State<'_, Requests>) -> Vec<TabRequest>;

/// Trong `setup`: mở kênh nghe, nhận `Opening` của chính mình, và trên macOS nghe deep-link.
pub fn start(app: &AppHandle, opening: Opening);
```

`launch::accept(app, url, secret)` là **chỗ duy nhất phía Rust ánh xạ URL → module**: host
`connect` → `modules::db::handoff::accept`. Đây là bản Rust của `shell/registry.ts`: một điểm nối,
đặt tên module đúng một lần. Host lạ → stderr, không tab.

Hàng đợi + sự kiện thay vì chỉ sự kiện: `Opening` của chính tiến trình được nhận trong `setup`,
**trước khi webview có listener nào** — một sự kiện phát lúc đó rơi vào khoảng không. Nên backend
luôn đẩy vào hàng đợi rồi phát `launch://request` không payload; frontend nghe sự kiện *rồi* rút
hàng đợi qua `launch_take_requests`, và rút một lần nữa ngay lúc mount. Không mất, không đúp.

### `instance.rs` — kênh giữa hai tiến trình

Một tin nhắn JSON một dòng, `{ "url": …, "secret": … }`, cả hai `Option`, kết thúc bằng `\n`; bên
nghe trả `ok\n`. Tiến trình thứ hai gửi rồi thoát 0. Không có `url` là "chỉ đưa cửa sổ lên trước".

| | Endpoint | Client | Server |
| --- | --- | --- | --- |
| Windows | `\\.\pipe\<identifier>` | `tokio::net::windows::named_pipe::ClientOptions::open` | `ServerOptions::first_pipe_instance(true)` — tạo thất bại (os error 5) nghĩa là đã có tiến trình khác giữ, không nghe nữa |
| macOS / Linux | `$XDG_RUNTIME_DIR` → `$TMPDIR` → `/tmp`, file `<identifier>.sock` | `std::os::unix::net::UnixStream::connect` | `tokio::net::UnixListener::bind`, xoá file lúc `RunEvent::Exit` |

```rust
pub struct Endpoint(String);           // từ identifier, hoặc từ tên bất kỳ cho test
pub async fn forward(endpoint: &Endpoint, line: &str) -> bool;              // true = có người nhận
pub async fn serve(endpoint: Endpoint, on_line: impl Fn(String) + Send + Sync + 'static);
```

Cả hai `async` để test được trong `#[tokio::test]`; `lib.rs` gọi `forward` qua
`tauri::async_runtime::block_on` với `timeout(3s)` — tiến trình thứ nhất treo thì tiến trình thứ hai
tự thành cửa sổ mới thay vì treo theo.

Trình tự khởi động của tiến trình thứ nhất: `connect` thử → `NotFound`/`ConnectionRefused` → xoá
file socket cũ (nếu có) → `bind`. Hai tiến trình khởi động cùng lúc: cái `bind` sau lỗi
`AddrInUse` → không nghe, và vì nó đã qua bước `forward` thất bại từ trước nên nó mở cửa sổ riêng —
hai cửa sổ trong một cuộc đua hiếm, không mất gì.

**Kẻ chiếm chỗ.** Trên Unix, trước khi `connect`, kiểm tra chủ file socket bằng `MetadataExt::uid`
so với `libc::getuid()`; không phải của mình thì không gửi, không xoá, chạy như tiến trình thứ nhất
không có kênh. Đây là lý do duy nhất thêm `libc` (chỉ `cfg(unix)`). Trên Windows, một tiến trình
khác trong phiên tạo pipe cùng tên trước là chiếm được — cùng lớp rủi ro với `FindWindow` của
plugin single-instance, và cùng câu trả lời: mô hình bảo mật của cả the standalone client lẫn MixEngine là một
máy một người dùng. Ghi vào phần rủi ro, không giải.

## 3. Vì sao là kênh riêng chứ không phải plugin

`tauri-plugin-single-instance` làm đúng hai việc: phát hiện tiến trình đang chạy và gửi `argv` sang.
Hợp đồng T83 cần việc thứ ba — gửi mật khẩu mà không đặt nó vào `argv` — và plugin không có chỗ cho
nó. Hai lựa chọn còn lại:

- **Plugin + không mật khẩu khi đã mở sẵn**: tab mở ra với form điền sẵn, ô mật khẩu trống. Đúng
  hợp đồng về mặt an toàn, sai về mặt mục đích: trường hợp phổ biến nhất của `mix database open` là
  the standalone client đang mở, và đó là trường hợp phải gõ mật khẩu.
- **Kênh riêng** (~200 dòng, hai nhánh OS, một test round-trip): tab mở ra và tự nối. Người dùng đã
  chọn cái này.

Kênh riêng cũng làm luôn việc của plugin (đưa cửa sổ lên trước khi chạy lần hai), nên plugin không
cần có mặt.

## 4. Frontend: shell mở tab theo yêu cầu, module nối

### Shell — `src/shell/launch.ts`

```ts
export interface TabRequest { moduleId: string; state: unknown }
export function parseTabRequest(value: unknown, knownModuleIds: string[]): TabRequest | null;  // thuần, test
export function takeTabRequests(): Promise<TabRequest[]>;      // invoke launch_take_requests → lọc bằng parse
export function onTabRequest(cb: () => void): Promise<UnlistenFn>;   // listen "launch://request"
```

`App.tsx`: `openTab(moduleId?, state?)` nhận thêm `state`, đặt vào `TabInfo.state` của tab mới —
đúng khe `restored` đã có; shell vẫn không đọc nó. Một effect lúc mount: nghe sự kiện, rồi rút hàng
đợi; mỗi yêu cầu → `openTab(moduleId, state)` và tab đó thành active. Shell **vẫn không có
`switch (moduleId)`**: `moduleId` chỉ được đối chiếu với `MODULES` như `parseSession` đang làm.

### Module db — `tabState.ts`, `handoff.ts`, `DbTab.tsx`

```ts
// tabState.ts
export type DbTabState = { savedId: string; connected: boolean } | { handoffId: string };
```

`parseDbTabState` nhận thêm dạng `{ handoffId }`. Vẫn **chỉ id**: uuid của một mục trong kho tạm
Rust, hết hạn ngay khi được lấy — có nằm trong `localStorage` cũng không trỏ tới gì.

```ts
// handoff.ts — chỗ duy nhất gọi invoke cho việc này, như tunnel.ts
export interface Handoff { config: ConnectionConfig; label: string }
export function takeHandoff(id: string): Promise<Handoff>;
```

`DbTab.tsx`, một effect bên cạnh effect khôi phục hiện có, cũng qua `restoreTried`:

1. `onStateChange(undefined)` **ngay** — tab này không trỏ tới gì bền, session không được giữ
   `handoffId`.
2. `takeHandoff(id)`. Thành công → `setForm(formFrom(config))`, `setSaveAsName(label)`,
   `onTitleChange(label)`, rồi hỏi `arrivesConnected(config)`:
   - **có mật khẩu, hoặc là Redis** (không có tài khoản) → `connect(config, label)` ngay. Đây là
     đường MixEngine.
   - **không có mật khẩu mà server có tài khoản** → không nối. Form đã điền đủ, dòng trạng thái nói
     "Nhập mật khẩu để kết nối", con trỏ đặt sẵn trong ô mật khẩu (`ConnectionForm` nhận
     `focusPassword`, một bộ đếm). Đây là đường link `<old>://` từ trình duyệt hay tài liệu: cùng
     một URL nhưng không có env đứng sau, và nối bằng mật khẩu rỗng chỉ để nhận "access denied"
     thì không giúp ai.

   Thất bại (hết hạn) → không làm gì: form trống, không banner, y như tab mở tay.

`connect` đi nguyên đường cũ: `connect_db`, workspace, badge kind. Nối lỗi (server chưa lên) → banner
lỗi trên form **đã điền đủ kể cả mật khẩu**, bấm Connect là thử lại — không có nhánh riêng nào cho
handoff sau điểm này. Disconnect → form vẫn giữ mọi thứ, Save lưu như thường.

## 5. Trình tự trong `lib.rs`

```rust
pub fn run() {
    // Dòng đầu, main còn đơn luồng: argv[1] và mật khẩu ra khỏi env.
    let opening = launch::Opening::from_process();
    let context = tauri::generate_context!();

    // Có the standalone client đang chạy → nó nhận, ta xong. exit 0 là "handed_on" với MixEngine.
    if launch::forward(&context.config().identifier, &opening) { return; }

    let builder = tauri::Builder::default()
        …các plugin cũ…
        .plugin(tauri_plugin_deep_link::init());
    let builder = launch::register(builder);          // Requests
    let builder = modules::db::register(builder);     // DbState + HandoffState
    …
    .setup(move |app| {
        launch::start(app.handle(), opening);        // nghe kênh; nhận opening; macOS: on_open_url; Linux: register_all
        …sweep_downloads như cũ…
    })
```

`launch::start`:

- `instance::serve` trên `tauri::async_runtime::spawn`; mỗi dòng nhận được → đưa cửa sổ `main` lên
  trước (`unminimize`, `show`, `set_focus`) rồi `launch::accept(app, url, secret)` nếu có `url`.
- `launch::accept` cho `Opening` của chính mình.
- `#[cfg(target_os = "macos")]`: `deep_link.on_open_url(|e| accept(url, None))` — Apple Event là
  đường duy nhất một URL tới được tiến trình macOS đang chạy hoặc vừa được `open` mở. `secret` luôn
  `None`: env của tiến trình đang chạy đã được rút từ dòng đầu, và một link không đặt được env.
- **Không** nghe `on_open_url` trên Windows/Linux — điều đo được số 2: plugin tự phát cho `argv`
  mà ta đã tự xử lý, và URL lúc đang chạy đến qua kênh riêng chứ không qua plugin.
- `#[cfg(target_os = "linux")]`: `deep_link.register_all()`, lỗi bỏ qua — mục 6.

## 6. Đăng ký scheme

`tauri.conf.json`:

```json
"plugins": { "deep-link": { "desktop": { "schemes": ["<old>"] } } }
```

Bundler đọc nó: NSIS ghi `HKCU\Software\Classes\<old>`, `Info.plist` có `CFBundleURLTypes`, `.deb`
có `MimeType=x-scheme-handler/<old>` trong `.desktop`. AppImage thì không có installer nào ghi gì,
nên chỉ trên Linux the standalone client gọi `register_all()` lúc chạy: nó ghi
`~/.local/share/applications/<old>-handler.desktop` và gọi `xdg-mime` — lỗi (không có `xdg-mime`,
thư mục không ghi được) bỏ qua, vì scheme là tiện ích chứ không phải điều kiện để app chạy.

Không cần entry trong `capabilities/default.json`: frontend không gọi lệnh nào của plugin.

MixEngine không cần scheme này (D1 bên đó: nó chạy thẳng binary, không hỏi OS ai sở hữu `<old>://`).
Đăng ký là để link trong tài liệu, trong trình duyệt hoạt động — và là lý do luật tên biến ở mục 1
phải có.

## 7. Cái không được đi đâu

| Mật khẩu | Ở đâu | Không ở đâu |
| --- | --- | --- |
| Tiến trình thứ nhất | env (đọc rồi xoá ở dòng đầu) → `Opening.secret` → `Handoff.config.password` trong `HandoffState` → webview (một lần, qua `handoff_take`) → `connect_db` | `argv`, stderr, `Debug` (`Opening`, `Handoff` đều che), `localStorage`, `connections.json` |
| Tiến trình thứ hai | env → `Opening.secret` → một dòng JSON trên pipe/socket → thoát | như trên, cộng: không sống quá vài chục ms |
| Link từ trình duyệt | không có | — |

`Opening` và `Handoff` có `Debug` viết tay dùng `Redacted`, và test khẳng định chuỗi `{:?}` không
chứa mật khẩu — cùng kiểu test `a_connection_never_prints_what_it_knows` đang có.

## 8. Kiểm thử

**Rust, thuần** (`cargo test`, chạy trong CI Linux):

- `handoff::parse`: URL đầy đủ → đúng năm trường và `label` đã decode `%40`; Redis tối thiểu →
  không `username`, không `password`; thiếu `kind`/`host`/`port`, `port` = 0 hoặc > 65535, `kind`
  lạ, scheme lạ, host không phải `connect` → `error.handoffInvalid`; `label` vắng → `host:port`.
- `handoff::credential_name`: `MIXENGINE_DB_PASSWORD` qua; `PATH`, `DB_PASSWORD`, `PGPASSWORD`,
  `mixengine_db_password` không.
- `Opening::from_args` với closure env giả: URL có `password_env` → closure được hỏi đúng tên, và
  đúng một lần; URL không có → không hỏi; `argv[1]` không phải `<old>://` → `url = None` và không
  hỏi; URL hỏng nhưng có `password_env` hợp lệ → vẫn hỏi (biến vẫn bị rút).
- `Debug` của `Opening` và `Handoff` không chứa mật khẩu.

**Rust, hệ điều hành thật** (`#[tokio::test]`, endpoint tên ngẫu nhiên): `serve` + `forward` một
dòng → `on_line` nhận đúng dòng đó và `forward` trả `true`; `forward` tới endpoint không ai nghe →
`false`, nhanh. Chạy trên cả ba OS qua `cargo test` — Windows và macOS do máy dev, Linux do CI.

**Frontend, vitest**: `parseTabRequest` — đúng hình, `moduleId` lạ → `null`, rác → `null`;
`parseDbTabState` — `{ handoffId }` được nhận, `{ handoffId: "" }` không, và `{ savedId, handoffId }`
đọc thành `savedId` (cái đã có thắng, để session cũ không đổi nghĩa).

**Bằng tay, ghi vào báo cáo cuối vì không tự động được** (cần MixEngine hoặc giả lập nó):

```powershell
$env:MIXENGINE_DB_PASSWORD = "…"
& "$env:LOCALAPPDATA\the standalone client\<old>.exe" "<old>://connect?kind=mysql&host=192.168.50.86&port=3307&user=root&label=mysql%4057&password_env=MIXENGINE_DB_PASSWORD"
```

Lần một: cửa sổ mở, tab `mysql@57` nối sẵn. Lần hai (đang mở): tab thứ hai trong cùng cửa sổ, tiến
trình thứ hai thoát 0 tức thì (`$LASTEXITCODE`). Mở terminal trong the standalone client, `echo
$env:MIXENGINE_DB_PASSWORD` → trống. `Get-Content connections.json` → không có mật khẩu.

## 9. Rủi ro và câu trả lời

| Rủi ro | Trả lời |
| --- | --- |
| Trang web phát `<old>://…&password_env=X` để đọc env người dùng | Mục 1: chỉ `MIX*_PASSWORD`; và biến đó chỉ tồn tại trong tiến trình MixEngine khởi động |
| Trang web phát `<old>://connect?host=…` để the standalone client nối tới host lạ | Không có mật khẩu thì không nối (mục 4): tab hiện form với host lạ trong đó, người dùng là người bấm Connect. Redis không có tài khoản thì vẫn nối — cái giá của một scheme công khai, và không có gì rò rỉ |
| Helper của WebView2 / shell trong terminal kế thừa mật khẩu | `remove_var` ở dòng đầu `run()`, trước builder |
| Tiến trình thứ hai bị coi là thất bại | Thoát 0 trong vài chục ms; `forward` có timeout 3s để không bao giờ treo quá "một giây phán xét" của MixEngine mà không có lý do |
| Cửa sổ thứ nhất treo | Timeout → tiến trình thứ hai tự mở cửa sổ; pipe/socket tạo thất bại → chạy không kênh |
| File socket cũ sau crash | Xoá khi `connect` báo `ConnectionRefused` |
| Pipe/socket bị chiếm chỗ bởi người dùng khác | Unix: kiểm chủ file; Windows: chấp nhận dưới mô hình một máy một người — ghi ở mục 2 |
| Hai tab cho một lần chạy trên Windows/Linux | Không nghe `deep-link://new-url` ở đó — mục 5 |
| `handoffId` cũ trong session | `handoff_take` trả `handoffExpired`, tab im lặng hiện form trống |
| MixEngine đổi mã hoá URL (`+` cho khoảng trắng) | `query_pairs` đọc `+` là khoảng trắng, `%2B` là `+` — hợp với cả hai kiểu bộ mã hoá |

## 10. Những gì để lại

- **Keyring chung với MixEngine** (T84 bên đó): khi nó có, `password_env` có thể vắng và the standalone client đọc
  mật khẩu từ kho OS theo `label`. Cấu trúc ở đây không cản: `secret: None` đã là một nhánh có sẵn.
- **`mongo` trong URL**: nếu một ngày MixEngine quản MongoDB, `parse` thêm một nhánh dựng `uri`
  từ năm trường. Không làm trước.
- **Tự lưu kết nối handoff**: hỏi người dùng có muốn Save không sau khi nối. Chưa ai xin.
