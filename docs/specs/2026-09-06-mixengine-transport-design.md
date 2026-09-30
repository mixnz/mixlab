---
status: implemented
date: 2026-09-06
---

# Nói chuyện với MixEngine: transport, và màn hình đầu tiên của module `mixengine`

Ngày 2026-09-06. Pha 1 của [roadmap/mixengine-module.md](../roadmap/desktop/mixengine-module.md).

MixEngine là môi trường web dev cục bộ chạy dưới dạng một daemon (`mixengined`) và **cố ý không có
GUI** — ADR 0011 bên đó. `mix` chỉ là client mỏng trên cùng một API. Repo này dựng client đồ họa ấy.

## Mục tiêu

- Một tab `mixengine` mở ra nhìn thấy daemon thật: version, protocol, uptime, home.
- Bảng service với trạng thái **sống** — đến từ stream sự kiện, không phải từ suy đoán.
- Bật, tắt, khởi động lại từng service, và dừng tất cả.
- Một thao tác cần quyền quản trị hiện **đầy đủ những gì nó sẽ đổi** trước khi UAC/sudo bật lên.
- Daemon chưa chạy là một trạng thái đọc được, không phải một lỗi đỏ.

## Phi mục tiêu

- **Không** Sites, Runtimes, Logs, Metrics, Blueprints, Extensions, Settings. Pha 2–4.
- **Không** gọi `mix` qua process. Lý do ở mục 2.
- **Không** đường mạng. Daemon không mở cổng TCP; `--listen` bên đó là thiết kế chưa xây.
- **Không** cài MixEngine hộ người dùng.
- **Không** đọc thẳng SQLite hay file config của MixEngine. Chỉ đi qua API.
- **Không** ghi vào namespace keyring `mixengine`. The standalone client chỉ đọc, và chỉ entry mà một handoff thật
  đã chỉ tới — luật đó đã chốt ở Pha 0.

## Hiện trạng

Ba thứ trong repo này đã sẵn và spec dựa lên cả ba.

**Repo đã biết nói named pipe và Unix socket.** [`instance.rs`](../../apps/desktop/src-tauri/src/instance.rs)
làm kênh giữa hai bản the standalone client: `ClientOptions`/`ServerOptions` trên Windows,
`UnixStream`/`UnixListener` ở nơi khác, cả hai qua tokio. **Không tái sử dụng nó** — nó chở một dòng
text mỗi chiều tới endpoint của chính mình, còn đây là HTTP/1.1 tới endpoint của người khác, có
stream sống hàng giờ. Cái đi mượn là **hình dạng của lát cắt `#[cfg]`**, không phải code.

**Đường handoff đã xong.** Pha 0 (`91abac3`, #20) dựng `<old>://connect`, `Handoff::keyring_ref` và
`secrets_resolve_mixengine`. Spec đó là
[2026-09-03-mixengine-connection-handoff-design.md](2026-09-03-mixengine-connection-handoff-design.md).
Ở đây nó là **chiều ngược lại**: MixEngine đẩy sang the standalone client, còn spec này là the standalone client hỏi MixEngine.

**`hyper` đã nằm trong cây.** `hyper 1.11` và `hyper-util 0.1.20` đã có trong `Cargo.lock` qua
reqwest và tauri; khai báo trực tiếp không thêm crate mới. `tokio` đã bật `full`, nên
`tokio::net::windows::named_pipe` có sẵn. `libc` đã là dependency chỉ-unix.

---

## 1. Địa chỉ endpoint

Thứ chặn mọi thứ khác: chưa biết địa chỉ thì không có gì để nói chuyện.

### Home

`MIXENGINE_HOME` nếu có, không thì mặc định theo hệ điều hành:

| | |
| --- | --- |
| Windows | `%LOCALAPPDATA%\MixEngine` |
| macOS | `~/Library/Application Support/MixEngine` |
| Linux | `$XDG_DATA_HOME/mixengine`, không có thì `~/.local/share/mixengine` |

### Unix

Socket là một file trong home: `<root>/run/mixengined.sock`, mode `0600`. Không có gì phải đoán, và
không cần kiểm gì thêm — file nằm trong `run/` mà chính tài khoản này sở hữu, không tài khoản khác
đặt một cái vào đó để bị tìm thấy nhầm được.

### Windows

```
\\.\pipe\mixengine.<SID của người dùng hiện tại>.<fingerprint>
```

`fingerprint` là **FNV-1a 64-bit** trên chuỗi đường dẫn `<root>/run` đã lowercase, in ra `{:016x}`:

```rust
const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME:  u64 = 0x0000_0100_0000_01b3;
run.to_string_lossy().to_lowercase().bytes()
    .fold(OFFSET, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(PRIME))
```

Lowercase vì đường dẫn Windows không phân biệt hoa thường: `--home C:\dev\sandbox` và
`MIXENGINE_HOME=c:\dev\sandbox` là một thư mục và phải tới một daemon.

**Đây là thứ đi mượn, và spec nói thẳng ra.** Thuật toán trên **không** nằm trong `bindings/` — nó
là chi tiết nội bộ của `mixengine-platform`. Chép lại nghĩa là the standalone client gánh một giả định mà CI bên kia
không canh hộ. Xem D1.

## 2. Transport, và vì sao không gọi `mix`

`mix --json` chạy được mọi lệnh mutating, nên cám dỗ là gọi nó qua process. Ba thứ giết phương án đó:
`GET /events`, `GET /logs/{id}` và `GET /metrics` đều là stream sống — một process gọi rồi thoát
không chở được cái nào; mỗi lần hỏi tốn một process; và nó phụ thuộc `mix` có trên `PATH`, thứ the standalone client
không kiểm soát.

Nên: `hyper` client HTTP/1.1 trên transport cục bộ, IO bọc bằng `hyper_util::rt::TokioIo`.

- **`POST /rpc`** — mở kết nối cho mỗi call rồi đóng. Trên socket cục bộ, chi phí bắt tay là micro
  giây; đổi lại không phải nuôi pool và không có kết nối chết nào phải phát hiện.
- **Ba stream** — một kết nối sống cho mỗi cái, đóng khi tab đóng.

### Kiểm chủ sở hữu pipe trước byte đầu tiên — chỉ Windows

Namespace pipe của Windows **phẳng và toàn máy**, tên suy ra được từ một SID công khai cộng một
fingerprint mà mã nguồn bên kia viết rõ, và `CreateNamedPipeW` không cần đặc quyền gì. Nên một tài
khoản khác trên cùng máy **giữ được cái tên đó trước khi daemon lên** và thu mọi request — kể cả
`elevation.*`. Cờ `FILE_FLAG_FIRST_PIPE_INSTANCE` của daemon chỉ chặn nó *nhập* vào pipe đó, không
chặn client dial nhầm.

Nên client đọc **owner của đối tượng pipe** và cúp máy trước byte đầu tiên nếu không phải tài khoản
này, hỏng với câu *"pipe đang do … giữ, không phải tài khoản này"* thay vì một timeout khó hiểu. Đọc
**owner**, không đọc process id: pid tái sử dụng được giữa lúc lấy và lúc tra, còn owner được đóng
dấu lúc tạo và không đặt thành tài khoản mà người tạo không có.

Đây là R1 trong review 2026-08-27 của MixEngine. Bỏ nó đi là để lại một lỗ thật.

Unix không cần: socket là file trong `run/` của chính tài khoản này.

## 3. `POST /rpc`, và lỗi

JSON-RPC 2.0. `method` là `namespace.verb`. Pha này gọi đúng bốn namespace: `daemon.*`, `service.*`,
`elevation.*`, `job.*`.

**HTTP status nói về phong bì; lỗi JSON-RPC nói về cuộc gọi.** Một method thất bại là `200` mang
member `error`. Các status thật sự xuất hiện đều là chuyện phong bì: `204` cho body toàn
notification, `400` body không đọc được, `404` route không có, `405` kèm `Allow`, `413` quá 1 MiB.

**Rẽ nhánh theo `error.data.code`, không bao giờ theo câu chữ.** Tập mã đóng:

```
not_found · already_exists · invalid_argument · conflict · precondition_failed
port_in_use · privileged_required · unsupported_platform · dependency_missing
process_failed · io · internal
```

`error.data.hint` là thứ UI vẽ thành hành động gợi ý — có thì hiện, không có thì thôi.

### Ánh xạ sang `AppError`

[`error.rs`](../../apps/desktop/src-tauri/src/error.rs) chở `{ code, params }` và frontend dịch. Bốn code mới,
thêm vào cả `en.ts` lẫn `vi.ts`:

| Code | Khi nào | Params |
| --- | --- | --- |
| `error.mixengineUnreachable` | không dial được endpoint | `endpoint` |
| `error.mixengineRefused` | daemon trả `error` | `code`, `message`, `hint` |
| `error.mixengineProtocol` | body không phải JSON-RPC hợp lệ, hoặc status lạ | `message` |
| `error.mixenginePipeOwner` | pipe do tài khoản khác giữ (Windows) | `owner` |

`message` của daemon **không dịch** — đó là daemon nói, và đó là chuỗi người ta tra cứu được. Đúng
luật `error.rs` đã đặt cho message của driver.

## 4. Stream sự kiện

`GET /events`, Server-Sent Events. Sự kiện **internally tagged**: một dòng `data:` chứa
`{"type": "…", …}`, không có dòng `event:`. Nghĩa là một handler switch theo `type`, và một biến thể
sinh ra ở phiên bản sau tới the standalone client cũ như một object bỏ qua được — nên **không được** ném lỗi khi gặp
`type` lạ.

Parser cần đúng bốn luật: dòng bắt đầu bằng `:` là comment (stream rảnh gửi mỗi 15 giây — đó là thứ
phân biệt kết nối sống với kết nối chết); dòng `data:` gom lại; dòng trống kết thúc một message;
dòng khác bỏ qua.

Byte chảy về UI qua **Tauri `Channel`**, đúng cách module terminal đã làm.

**Sự kiện là best-effort và không bao giờ là đường duy nhất biết trạng thái.** Hai chỗ phải xử:

- `{"type":"resync","missed":N}` — bus 1024 message đã tràn. Gọi lại `service.list` và
  `daemon.status`. Con số `missed` chỉ để ghi log, không để rẽ nhánh: cách xử lý giống nhau dù lỡ
  một hay một nghìn.
- Kết nối đứt — gọi lại cả hai rồi mở stream mới.

## 5. Daemon chưa chạy

`GET /health` không cần xác thực, đúng để quyết định có tự khởi động không. Ba trạng thái phải phân
biệt được, và UI vẽ ba thứ khác nhau:

| | |
| --- | --- |
| **Không chạy** | không dial được endpoint → nút "Khởi động MixEngine" |
| **Không trả lời** | dial được, `/health` không xong → câu "daemon không trả lời", nút thử lại |
| **Không có MixEngine** | không tìm thấy `mixengined` → nơi đã tìm + link trang cài đặt |

Khởi động: `mixengined --detach`. Nó **chỉ trả về khi daemon đã trả lời trên endpoint** và in
endpoint ra stdout — nên **không viết vòng lặp backoff ở client**; việc chờ thuộc về tiến trình biết
con nó còn sống hay không.

**Bắt buộc đi qua `crate::platform::hide_console`** — [spawning-processes](../standards/desktop/spawning-processes.md).
Thiếu nó Windows bật một cửa sổ console đen trước mặt người dùng.

Tìm `mixengined` ở đâu: installer của MixEngine đặt thư mục của nó lên `PATH` người dùng, nên
`Command::new("mixengined")` là đường thường. Không thấy thì đó là trạng thái thứ ba ở bảng trên,
không phải một lỗi.

## 6. Types đi mượn, không tự viết

`bindings/` bên MixEngine là TypeScript sinh bằng ts-rs từ `mixengine-proto`, CI canh, và publish
thành `mixengine-api-<version>-typescript.tar.gz` ký cùng khóa với binary.

Vendor nguyên xi vào `src/modules/mixengine/api/types/`, kèm một dòng ghi version, **không sửa tay**.
Đây là ngoại lệ có ý thức với `modules/db/types.ts` (chép tay theo models Rust): ở đó không ai canh
hộ, ở đây có. Xem D2.

Phía Rust vẫn khai `serde` struct riêng cho những gì nó thật sự đọc — Rust không đọc file TypeScript
được, và một struct chỉ khai field mình dùng thì một member mới thêm vào response không làm vỡ nó
(ADR 0019 bên đó: member thêm là optional, protocol version không bump vì nó).

## 7. Frontend

```
src/modules/mixengine/
  index.ts            ModuleDefinition
  MixEngineTab.tsx    Cổng ba trạng thái ở mục 5 -> sidebar
  api.ts              Chỗ duy nhất gọi invoke() của module này
  api/types/          bindings/ vendor nguyên xi
  tabState.ts         parseMixEngineTabState — màn hình đang mở. Ids only.
  screens/Dashboard/  Pha này có đúng một màn hình
  components/ i18n/ mixengine.css
```

Một dòng trong [`registry.ts`](../../apps/desktop/src/shell/registry.ts), một dòng trong
[`dicts.ts`](../../apps/desktop/src/i18n/dicts.ts). Root của workspace cần đủ khối năm thuộc tính ở
[workspace-root](../standards/desktop/workspace-root.md).

**Sidebar dựng sẵn cho chín màn hình, Pha này bật một.** Tám mục còn lại không hiện — một mục xám
không bấm được là một lời hứa UI không giữ được.

### Dashboard

`daemon.status` cho version, protocol, pid, home, uptime. `service.list` cho một hàng mỗi service:
`id`, `state`, `supervised`, `pid`, `port`, `last_started_at`, `last_exit_code`, `depends_on`.

**Trạng thái đến từ stream, không từ suy đoán.** Bấm Start thì hàng đó chuyển sang `Starting…` **khi
`service_state_changed` nói vậy**, không phải ngay lúc bấm. Một công tắc nói dối về việc MariaDB có
đang chạy hay không tệ hơn một công tắc chậm.

`service.start/stop/restart` là ngoại lệ duy nhất nhận `wait` thay vì trả job — thời gian chờ bị
chặn bởi ready timeout do recipe của chính service khai, và mọi bước đã ở trên stream nên một call
đang chờ không bao giờ là một call mù. Pha này gửi `wait: true`.

## 8. Elevation là một luồng, không phải một lỗi

`{"type":"elevation_required","pending":[…]}` mang **mọi** thao tác đang chờ, cũ nhất trước, mỗi cái
kèm thứ nó sẽ đổi cụ thể: đúng những dòng `hosts`, đúng cổng, đúng kho tin cậy. `PrivilegedOp` có 13
biến thể (`probe`, `hosts-apply`, `port-access-grant`/`revoke`, `resolver-apply`/`revoke`,
`trust-ca-install`/`remove`, `firewall-apply`, `helper-install`/`replace`/`remove`,
`audit-log-remove`).

UI hiện danh sách đó **rồi mới** gọi `elevation.grant`, thứ bật đúng một prompt cho cả lô. Từ chối
là một kết cục API mô hình hóa được, không phải lỗi — `elevation.drop` là đường ra cho thao tác
người dùng không định cho phép. Máy mà không ai từng grant thì ở chế độ suy giảm mãi mãi, và **đó là
đúng**: `daemon.status` nói ra điều đó qua `ElevationSummary { elevated, can_prompt, pending }`.

Daemon **không bao giờ** tự bật prompt. Chỉ client gọi `grant`. Đó chính là thứ làm cho "giải thích
trước khi xin" nói ra được.

## 9. Job

Mọi thao tác dài trả `JobSummary { id, kind, state, percent, message, started_at, finished_at,
outcome }`; tiến độ tới qua `job_progress` / `job_finished`. Pha này chưa có thao tác dài nào của
riêng nó, nhưng hạ tầng job phải có: `daemon.doctor_repair` và `elevation.grant` đều đẻ ra job, và
Pha 3 sẽ dựng lên trên nó.

Vẽ trạng thái **ngay trên hàng**, không phủ spinner lên cả màn hình. `job.wait` là method duy nhất cố
ý chờ và có timeout — không dùng ở đây.

## 10. Kiểm thử

Phần thuần, không cần daemon nào — đây là chỗ đặt gần hết giá trị:

| Test | Nội dung |
| --- | --- |
| `fingerprint` | vài đường dẫn cố định ra đúng hex; hoa/thường ra cùng kết quả |
| `home` | `MIXENGINE_HOME` thắng mặc định; ba mặc định đúng cho ba OS |
| Parser SSE | comment `:` bị bỏ; nhiều `data:` gom lại; dòng trống chốt message; `type` lạ không ném |
| Ánh xạ lỗi | `200` + `error` ra `error.mixengineRefused` với `code`/`hint`; `404` ra `error.mixengineProtocol` |
| `resync` | sinh ra đúng hai lần gọi lại |
| `tabState` | `parseMixEngineTabState` nuốt được rác từ `localStorage` |

Cái **không** test được bằng vitest hay `cargo test`, và spec ghi ra để không ai tưởng đã phủ: tên
pipe có dial đúng daemon thật không, và `mixengined --detach` có thật sự trả về sau khi daemon trả
lời không. Cả hai chỉ `npm run dev:app` với MixEngine thật mới nói.

## 11. Rủi ro

- **Fingerprint lệch.** Chuỗi đường dẫn khác một dấu phân cách hay một dấu `/` cuối là ra pipe khác,
  và triệu chứng là "không tìm thấy daemon" chứ không phải một lỗi chỉ đúng chỗ. Giảm bằng: dựng
  đường `run` bằng `PathBuf::join` rồi mới `to_string_lossy`, một test ghim vài giá trị, và câu lỗi
  in ra **tên pipe đã tính** để so bằng mắt được.
- **Bên kia đổi cách đặt tên.** Không có CI nào bắt được. D1 là chỗ trả lời.
- **`mixengined` không trên `PATH`.** Bản zip trên Windows giải nén đâu cũng được. Rơi vào trạng
  thái thứ ba ở mục 5, không phải crash.
- **Daemon phiên bản cũ hơn `bindings/` đã vendor.** Member thiếu là optional theo ADR 0019, nên
  struct Rust chỉ khai field mình dùng sẽ sống. `daemon.version` và `/health` không bao giờ mọc thêm
  field — chúng là thứ đọc trước khi biết có tin phần còn lại được không.

## 12. Quyết định

**D1 — Tên pipe Windows: chép, và ghim.** Chép FNV-1a vào `transport.rs` kèm test giá trị cố định
và một comment nói rõ đây là thứ mượn của `mixengine-platform`, không phải hợp đồng. Song song, mở
issue bên `mixnz/mixengine` xin đưa địa chỉ endpoint vào phần publish — hoặc một file
`<root>/run/endpoint` đọc được, hoặc một dòng trong `index.json`. Chờ họ trước khi làm sẽ chặn cả
Pha 1 vì một thứ tự giải quyết được trong hai chục dòng.

**D2 — Vendor `bindings/` bằng script.** `scripts/` đã là chỗ repo này để việc kiểu đó
(`npm run notes`, `npm run set-version`, `npm run icons`). Thêm `npm run bindings` tải tarball của
một version, giải vào `src/modules/mixengine/api/types/`, ghi version ra một file cạnh đó. Không
submodule: nó bắt mọi người clone thêm một repo cho một thư mục type. Không chép tay: đúng cái ta
vừa nói là không nên.

**D3 — Dashboard Pha 1 dừng ở trạng thái, không có số đo.** Bảng service + `daemon.status` +
start/stop/restart + stop-all + luồng elevation. CPU/RSS chờ Pha 4, vì chúng cần `GET /metrics` và
cả bộ luật "phút thiếu nghĩa là không ai đo".

**D4 — Không tự khởi động daemon khi mở tab; hiện một nút.** Mở một tab là một cử chỉ rẻ, và người
dùng có thể chỉ đang tìm nhầm tab. Khởi động một daemon giám sát database thì không rẻ như vậy. Nút
"Khởi động MixEngine" nói rõ nó sắp làm gì. Nếu dùng thật thấy phiền, đảo lại ở Pha 2 là một dòng —
đảo theo chiều kia thì không.
