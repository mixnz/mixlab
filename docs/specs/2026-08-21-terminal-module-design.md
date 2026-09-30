---
status: implemented
date: 2026-08-21
---

# Module Terminal

Ngày: 2026-08-21

## Mục tiêu

The standalone client là một shell cộng các module. Hôm nay có hai: `db` và `rest`. Spec này mô tả module thứ ba —
một terminal, mở được phiên shell trên máy đang chạy app hoặc trên một máy chủ qua SSH — sống cạnh
hai module kia mà không bên nào biết khái niệm của bên nào.

Sau khi làm xong:

- Mở tab Terminal từ menu `[+]`, chọn shell cục bộ hoặc một host SSH đã lưu, và có một phiên gõ
  được: `vim`, `top`, `htop`, prompt màu, chuột, đổi kích thước đều chạy đúng.
- Host SSH lưu lại giữa các lần mở app; mật khẩu và passphrase nằm trong kho thông tin đăng nhập
  của hệ điều hành, không nằm trong file JSON.
- `Ctrl+W` và `Ctrl+R` khi con trỏ ở trong terminal đi xuống đầu xa chứ không đóng tab và reload
  pane; `Ctrl+C` ngắt tiến trình; copy/paste và tìm kiếm trong scrollback có sẵn.
- Không file nào ngoài `src/modules/terminal/` biết khái niệm nào của terminal, trừ đúng hai dòng
  mà [adding-a-module](../standards/desktop/adding-a-module.md) cho phép.

## Phi mục tiêu

Ghi ra để không bị kéo vào:

- **Không split pane.** Một tab là một phiên. Muốn hai phiên thì mở hai tab, đúng như module db.
- **Không SFTP**, không duyệt file, không kéo thả file lên máy chủ.
- **Không session restore.** Phiên chết khi thoát app, giống mọi tab khác của standalone-client.
- **Không tự kết nối lại.** Có lý do, xem mục 4.
- **Không ghi log phiên ra file**, không phát lại phiên.
- **Không snippet, không lệnh chạy sẵn khi vào phiên**, không profile theo màu.
- **Không đọc `~/.ssh/config`.** Host nhập trong app, lưu trong app — đúng như `known_hosts.json`
  là file riêng của app chứ không phải file của OpenSSH.
- **Không dùng chung host SSH với module db.** Ranh giới module cấm, và một tunnel với một phiên
  shell không cùng vòng đời — xem mục 1.
- **Không thêm jsdom hay test component.** Repo cố ý chỉ test logic thuần.

## Hiện trạng

Những gì đã có sẵn và spec này dựa vào:

| Chỗ | Dùng để làm gì |
| --- | --- |
| `src/shell/module.ts` | `ModuleDefinition` — id, Icon, Tab, `settings`, `shortcuts`, `TabBadge` |
| `src/shell/registry.ts` | Một dòng nữa trong `MODULES` |
| `src/i18n/dicts.ts` | Gộp từ điển của module, gồm nhóm `error` gộp tay |
| `src-tauri/src/ssh/mod.rs` | `SshConfig`/`SshAuth`, `authenticate()`, `known_hosts.json` |
| `src-tauri/src/secrets.rs` | `secrets_save`/`secrets_load`/`secrets_delete`, id là chuỗi bất kỳ |
| `src/modules/db/savedConnectionsStore.ts` | Pattern store dùng chung giữa mọi tab |
| `src/modules/rest/workspace.ts` | Pattern lưu cài đặt của một module |
| `src/components/` | `Button`, `Input`, `Select`, `ItemList`, `ErrorBanner`, `ContextMenu` |
| `src/core/shortcuts/` | `decide()` — và luật "handler đăng ký sau cùng thắng" |
| `src/core/textEntry.ts` | `isTextEntry` trả `true` cho `<textarea>` — xterm gõ vào một textarea ẩn |
| `src/icons/icons.tsx` | `TerminalIcon` đã có sẵn |

Ba điều kiểm được trong code, quyết định thiết kế bên dưới:

1. **`ssh/mod.rs` đã có phần khó nhất.** `authenticate()` ([dòng 303](../../apps/desktop/src-tauri/src/ssh/mod.rs#L303))
   kết nối, kiểm vân tay theo `known_hosts.json`, rồi xác thực bằng mật khẩu hoặc khoá riêng. Đang
   là private vì chỉ tunnel gọi. Phiên shell cần đúng nó.
2. **Backend đang stream bằng `app.emit`.** Module db bắn sự kiện toàn cục rồi frontend lọc theo
   tên. Với terminal thì mỗi phiên là một luồng byte riêng, nên dùng `tauri::ipc::Channel` — thứ
   Tauri 2 sinh ra cho đúng việc này — thay vì thêm một tên sự kiện toàn cục nữa.
3. **`isTextEntry` trả `true` cho textarea.** Nghĩa là khi con trỏ ở trong terminal, mọi chord có
   `whenTyping: "ignore"` đã tự được để yên. Phần còn lại phải xử lý bằng tay — xem mục 3.

## 1. Backend

Cây thư mục `src-tauri/src/modules/terminal/`:

```
mod.rs        register() — manage(TerminalState)
models.rs     TerminalTarget, TerminalSize, TerminalEvent, LocalShell
state.rs      TerminalState { sessions: Mutex<HashMap<String, Session>> }
commands.rs   terminal_open / write / resize / close / local_shells
local.rs      spawn() qua portable-pty
remote.rs     spawn() qua russh
```

### Trừu tượng là một struct, không phải trait

Pty cục bộ là IO chặn — `portable-pty` đọc bằng `std::io::Read` trên một thread riêng — còn SSH là
async trong tokio. Ép cả hai vào một `#[async_trait] trait` sẽ phải bọc thread vào async ở một đầu
và giả vờ ở đầu kia. Thay vào đó hai hàm `spawn` trả về cùng một tay cầm:

```rust
pub struct Session {
    /// Byte người dùng gõ, chảy tới đầu xa.
    input: mpsc::Sender<Vec<u8>>,
    /// cols/rows mỗi khi khung đổi kích thước.
    resize: mpsc::Sender<TerminalSize>,
    /// Đóng tab, hoặc app thoát.
    kill: CancellationToken,
}
```

`local::spawn(opts, out) -> Session` và `remote::spawn(ssh, opts, out) -> Session`. Chỗ khác nhau
giữa hai loại phiên chỉ nằm trong hai hàm này; từ `commands.rs` trở lên không còn phân biệt. Mỗi
`spawn` tự dựng bộ đọc của nó — một thread cho local, một task cho remote — và đẩy vào `out`.

`Session` bị `Drop` thì `kill` được huỷ, tiến trình con bị giết và channel SSH bị đóng. Nên đóng
tab, hay thoát app, đều không để lại phiên sót.

### Lệnh

| Lệnh | Nhận | Trả |
| --- | --- | --- |
| `terminal_open` | `id`, `target`, `size`, `on_event: Channel<TerminalEvent>` | `()` |
| `terminal_write` | `id`, `data` (chuỗi) | `()` |
| `terminal_resize` | `id`, `cols`, `rows` | `()` |
| `terminal_close` | `id` | `()` |
| `terminal_local_shells` | — | `Vec<LocalShell>` |

`id` do frontend sinh (uuid), một cái cho mỗi phiên.

```rust
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TerminalTarget {
    Local { shell: Option<String>, cwd: Option<String> },
    Ssh(crate::ssh::SshConfig),
}
```

`terminal_local_shells` dò xem máy có gì, để form khỏi bắt người dùng gõ đường dẫn: trên Windows là
PowerShell, `pwsh`, `cmd.exe`, Git Bash và các bản phân phối WSL; trên macOS và Linux là `$SHELL`,
`zsh`, `bash`, `sh`. Mỗi mục có nhãn hiển thị và đường dẫn thật.

### Byte chảy về UI

`terminal_open` nhận một `Channel` và mọi thứ đầu xa nói ra đi qua đúng kênh đó — mỗi phiên một
kênh, không phải lọc theo tên sự kiện toàn cục. Kênh chở hai loại khung, và cùng một kênh nên
**thứ tự được giữ**: `Exit` chắc chắn tới sau byte cuối cùng. Tách "byte thô một đường, exit một đường" thì sẽ có ngày dòng `logout` hiện ra sau khi
tab đã báo phiên đóng.

- `Data` là `InvokeResponseBody::Raw` — byte thô, tới JS thành `ArrayBuffer`.
- `Exit` là `InvokeResponseBody::Json`:

```rust
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TerminalEvent {
    Exit { code: Option<i32>, message: Option<String> },
}
```

Base64 từng là đường lui phòng khi `Channel` không nhận byte thô. Không cần nữa:
`JavaScriptChannelId::channel_on` đánh số thứ tự cho mọi khung và `Channel` phía JS xếp lại theo
số đó, nên đường `fetch` mà khung lớn phải đi không làm sai thứ tự. Chiều lên cũng bỏ base64:
`terminal_write` nhận `String`.

### Gom lô

Một lệnh `yes` bắn từng byte một qua IPC sẽ làm nghẹt webview. Bộ đọc gom vào đệm và chỉ gửi khi
**đủ 64KB hoặc hết 5ms**, tuỳ cái nào tới trước. Đây là thứ phải có ngay từ đầu chứ không phải tối
ưu để dành: không có nó thì `cat` một file log là treo app.

### Phần dùng chung ở `ssh/`

- `authenticate()` nâng từ private lên `pub(crate)`.
- Thêm `ssh::open_shell(config, app_data, size) -> Result<russh::Channel<client::Msg>, AppError>`:
  kết nối, xác thực, `request_pty` (`xterm-256color`, cols/rows ban đầu), `request_shell`.

Nằm ở `ssh/` chứ không trong module vì `known_hosts.json` là dữ liệu của app —
[backend.md](../architecture/desktop/backend.md) đã ghi trước điều này từ khi chỉ có module db.

Phiên terminal mở **kết nối SSH riêng của nó**, không dùng chung pool của tunnel. Vòng đời một
terminal là vòng đời cái tab; vòng đời một tunnel là vòng đời kết nối database. Gộp lại thì đóng
tab terminal làm rụng kết nối database, hoặc ngược lại.

## 2. Frontend

Cây thư mục `src/modules/terminal/`:

```
index.ts               terminalModule
TerminalTab.tsx        form chọn đích -> một phiên
types.ts               gương của models.rs
api.ts                 invoke(...) — chỗ duy nhất gọi backend
savedHosts.ts          tách/ghép secret, đọc ghi terminal-hosts.json
savedHostsStore.ts     danh sách dùng chung giữa mọi tab
settings.ts            cài đặt hiển thị, dùng chung giữa mọi tab
shortcuts.ts           TERMINAL_SHORTCUTS
terminal.css           style toàn cục của module
i18n/{en,vi}.ts
components/TerminalView/      instance xterm
components/TargetForm/        chọn local hoặc SSH
components/SearchBar/         Ctrl+F
components/TerminalSettings/  pane trong Settings của app
```

### TerminalTab

Đi đúng đường `DbTab` đã đi. Chưa có phiên thì hiện màn hình chọn đích: danh sách host đã lưu bên
trái, form bên phải với hai kiểu — `local` (chọn shell từ `terminal_local_shells()`, tuỳ chọn thư
mục bắt đầu) và `ssh` (host, port, user, auth bằng mật khẩu hoặc khoá riêng, đúng hình dạng
`SshConfig` mà Rust đã có). Kết nối xong, phiên chiếm cả tab.

`onTitleChange` đặt tiêu đề là `user@host` hoặc tên shell. `onBadgesChange` gắn một badge mang biểu
tượng local/SSH; phiên kết thúc thì badge đổi và `tabClassName` làm nhạt tab đi.

### Host đã lưu

Dùng lại nguyên cách chia của module db: `terminal-hosts.json` qua `plugin-store` giữ phần đọc được
(tên, host, port, user, đường dẫn khoá), còn mật khẩu và passphrase đi vào keyring qua ba lệnh
`secrets_*` đã có sẵn dùng chung. Id là uuid riêng của module nên không đụng id kết nối database.

`savedHostsStore.ts` — `useSyncExternalStore`, đọc một lần, mọi tab thấy cùng một danh sách — là
bản sao khoảng 60 dòng của `savedConnectionsStore.ts`. Chép có chủ đích chứ không tách ra `core/`:
đây mới là chỗ thứ hai, và `savedConnections` còn dính chuyện tách secret riêng của nó. Chỗ thứ ba
xuất hiện thì tách, không sớm hơn.

### TerminalView

Giữ instance xterm và ba đường nối:

- `term.onData` -> `terminal_write`
- `ResizeObserver` -> `fit()` -> `terminal_resize`, chặn dội ~100ms
- `Channel.onmessage` -> `term.write(...)`, hoặc hiện dải kết thúc phiên

Hai chi tiết dễ sai, vì tab nằm sau vẫn còn mounted:

- **Không được `fit()` khi khung đang ẩn.** Kích thước bằng 0 thì cols/rows tính ra là rác và bị
  bắn xuống server. Nhớ refit khi `active` trở lại.
- **`terminal_close` và `dispose()` chỉ chạy khi unmount**, không phải khi mất `active`. Tab nằm
  sau vẫn phải nhận byte và cuộn tiếp.

Phụ thuộc npm mới: `@xterm/xterm`, `@xterm/addon-fit`, `@xterm/addon-search`.
Phụ thuộc Rust mới: `portable-pty` — dùng ConPTY trên Windows, nên không phải viết code riêng cho
từng hệ điều hành.

### Cài đặt hiển thị

Pane trong Settings của app: font, cỡ chữ, số dòng scrollback, kiểu con trỏ và nhấp nháy, shell mặc
định khi mở tab cục bộ. Lưu trong `terminal-settings.json` theo đúng cách `rest/workspace.ts` giữ
bốn công tắc của nó — một store dùng chung, đổi ở Settings thì các phiên đang mở áp dụng ngay.

## 3. Bàn phím

xterm gõ vào một `<textarea>` ẩn, mà `isTextEntry` trả `true` cho textarea, nên mọi chord có
`whenTyping: "ignore"` đã tự động được để yên. Còn lại bốn chord của shell vẫn cướp phím khi con trỏ
ở trong terminal, và hai trong số đó là phím dùng liên tục trong shell:

| Chord | App đang hiểu là | Shell hiểu là | Quyết định |
| --- | --- | --- | --- |
| `Ctrl+W` | đóng tab | xoá một từ | về đầu xa |
| `Ctrl+R` | reload pane | tìm ngược trong lịch sử | về đầu xa |
| `Ctrl+T` | mở tab mới | (hiếm dùng) | để cho app |
| `Ctrl+1..n` | mở tab của module | (hiếm dùng) | để cho app |

Cách xử lý không cần sửa gì trong `shell/`: module đăng ký chord trùng khoá của chính nó
(`terminal.sendCtrlW`, `terminal.sendCtrlR`) và việc chúng làm chỉ là đẩy byte xuống đầu xa.
`decide()` khi có hai handler tranh một chord thì chọn cái **đăng ký sau cùng** — tức pane terminal
đang sống — đúng cơ chế `rest.closeRequest` đang dùng với `Ctrl+W` hôm nay. Và chỉ đăng ký khi tab
`active` và phiên đang mở, nên một cửa sổ không có terminal nào vẫn đóng tab và reload như cũ.

`Ctrl+C` không nằm trong danh mục chord nào, nên rơi thẳng xuống xterm và thành `0x03` = SIGINT.
Đúng như mong muốn, không phải làm gì. Vì vậy:

- `Ctrl/⌘+Shift+C` sao chép, `Ctrl/⌘+Shift+V` dán — quy ước của terminal.
- `Ctrl/⌘+F` mở thanh tìm kiếm trong scrollback (`SearchAddon`).

Cả ba đăng ký như chord của module để hiện trong bảng phím tắt ở Settings. Trên macOS quy ước thật
ra là ⌘C trơn, nhưng `Chord` hiện không diễn đạt được "chord này khác nhau theo hệ điều hành"; đợt
đầu để thống nhất, thêm ⌘C/⌘V trơn cho mac sau nếu dùng thấy vướng.

Chuột phải mở `ContextMenu` dùng chung mà module db đang dùng — sao chép, dán, xoá màn hình, chọn
tất cả — vì `nativeContextMenu.ts` sẽ cho menu của webview hiện lên trên textarea của xterm nếu
module không chặn.

## 4. Lỗi

Đi đúng đường `AppError` + macro `err!` với khoá dịch; module thêm nhóm `error.*` của riêng nó vào
phần gộp tay trong `i18n/dicts.ts`.

| Hỏng ở đâu | Người dùng thấy gì |
| --- | --- |
| Xác thực SSH sai | `ErrorBanner` ngay tại form, phiên không mở |
| Vân tay host đổi | Thông báo của `ssh/mod.rs`, có sẵn |
| Không tìm thấy shell cục bộ | `ErrorBanner` tại form |
| Pty không spawn được | `ErrorBanner` tại form |
| Mất mạng giữa phiên | `TerminalEvent::Exit`, dải dưới màn hình, nội dung đã cuộn giữ nguyên |
| Shell thoát bình thường | Cùng dải đó, kèm mã thoát |

**Phiên terminal không tự kết nối lại.** Tunnel SSH thì có — nhánh trước vừa làm việc đó — nhưng
một tunnel không mang trạng thái, còn một shell thì có: thư mục hiện tại, biến môi trường, chương
trình đang chạy dở. Mở lại lặng lẽ sẽ cho ra một shell mới trông y hệt shell cũ, và người dùng gõ
tiếp vào một chỗ không phải chỗ họ nghĩ. Phiên chết thì nói là chết, kèm nút "Kết nối lại" bấm tay.

## 5. Kiểm thử

`npm test` chỉ chạy logic thuần, nên phần vào vitest là:

- `savedHosts.ts` — tách secret ra rồi ghép lại có đi đúng vòng tròn không.
- `settings.ts` — gộp giá trị mặc định với giá trị đã lưu, kể cả file cũ thiếu trường.
- Tranh chấp phím — với `ALL_SHORTCUTS` và pane terminal đăng ký sau cùng, `decide()` phải trả
  `terminal.sendCtrlW` chứ không phải `app.closeTab`.
- Bản đồ tên shell -> nhãn hiển thị.

Bên Rust: unit test cho bộ gom lô — đủ 64KB thì đẩy, hết 5ms thì đẩy, rỗng thì không đẩy.
`ssh/mod.rs` đã có test kiểu này nên không phải dựng gì mới.

Phần không test nào nói hộ được thì kiểm tay qua `npm run dev:app`:

- `vim` và `top` qua SSH; kéo cửa sổ, chương trình toàn màn hình vẽ lại đúng.
- `yes` chạy vài giây — UI không nghẹt.
- `Ctrl+C` ngắt được; `Ctrl+W` xoá từ; `Ctrl+R` mở tìm ngược của bash.
- Đóng tab thì tiến trình chết thật (soi bằng `ps` / Task Manager); thoát app không sót phiên nào.
- Menu `[+]` mở được cả ba module.

Cộng `npm run build` và hai lệnh grep kiểm ranh giới trong `adding-a-module.md`.

## 6. Chia đợt

Theo đúng cách module REST đã đi: một spec chung, mỗi đợt một plan riêng.

1. **Khung + shell cục bộ.** Module Rust, `Session`, `local.rs`, năm lệnh, `Channel` + bộ gom lô;
   phía FE là khung module, một dòng trong `registry.ts`, `TerminalView` với xterm, form chỉ có
   kiểu local, i18n, tiêu đề và badge. Câu hỏi byte thô hay base64 được trả lời ngay ở đợt này.
2. **SSH.** `ssh::open_shell`, `remote.rs`, nhánh SSH của form, host đã lưu + keyring.
3. **Bàn phím và văn bản.** `Ctrl+W`/`Ctrl+R` về đầu xa, copy/paste, `Ctrl+F`, menu chuột phải.
4. **Settings pane** và phần vuốt lại: font, scrollback, con trỏ, shell mặc định.

Đợt 1 và 2 là thứ khiến module dùng được; đợt 3 là thứ khiến nó dùng được lâu.

Mỗi đợt tự viết dòng của nó vào `## [Unreleased]` trong CHANGELOG.md, theo
[changelog](../standards/desktop/changelog.md).

## 7. Rủi ro

| Rủi ro | Xử lý |
| --- | --- |
| ~~`Channel` không nhận byte thô~~ | Đã kiểm trong mã nguồn tauri 2.11.5: nhận, và giữ thứ tự bằng `index`. Bỏ base64 |
| ConPTY trên Windows vẽ sai | `portable-pty` là thứ WezTerm dùng thật; kiểm bằng `vim` ngay đợt 1 |
| Thông lượng cao làm nghẹt UI | Gom lô ngay từ đợt 1, kiểm bằng `yes` |
| `portable-pty` kéo theo nhiều phụ thuộc | Kiểm kích thước bundle sau đợt 1 |
