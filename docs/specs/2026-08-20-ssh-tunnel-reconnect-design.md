---
status: implemented
date: 2026-08-20
---

# SSH tunnel tự phục hồi

Ngày: 2026-08-20

## Mục tiêu

Một connection đi qua SSH tunnel, để yên một lúc lâu rồi quay lại, vẫn dùng được tiếp — không
phải bấm Disconnect rồi kết nối lại.

Sau khi làm xong:

- Phiên SSH chết thì tunnel tự mở phiên mới **sau lưng cùng một cổng local**, nên pool của sqlx,
  `ConnectionManager` của Redis và client của Mongo tự khỏi mà không cần biết chuyện gì đã xảy ra.
- Keepalive giữ phiên sống qua NAT và qua `ClientAliveInterval` của sshd, đồng thời phát hiện
  đường đã chết trong khoảng 45 giây thay vì tại lệnh kế tiếp người dùng bấm.
- Người dùng thấy chuyện đang xảy ra: một banner *đang kết nối lại* / *đã kết nối lại* / *thất bại*
  kèm nút thử lại, thay vì một câu tiếng Anh của sqlx.
- Lệnh **đọc** gặp đúng lúc đứt được chạy lại một lần sau khi tunnel hồi phục. Lệnh **ghi** thì
  không: nó báo lỗi và để người dùng quyết định.
- `MySQL: error communicating with database: expected to read 4 bytes, got 0 bytes at EOF` không
  còn lọt ra giao diện dưới dạng nguyên văn của driver.

## Phi mục tiêu

Ghi ra để không bị kéo vào:

- **Không tự chạy lại lệnh ghi.** Một `INSERT` chạy lại sau khi mất kết nối có thể là hai dòng:
  câu lệnh có thể đã tới máy chủ và chỉ có câu trả lời là mất. Đây là quyết định đã chốt.
- **Không tự reconnect pool bằng tay.** sqlx, redis và mongo đều đã làm việc đó; thêm một lớp nữa
  chỉ là hai thứ cùng cố sửa một vấn đề.
- **Không thay russh, không gọi `ssh` ngoài hệ thống.**
- **Không đụng known-hosts / TOFU.** Phiên mở lại đi qua đúng `TunnelHandler` cũ, nên máy chủ đổi
  khoá giữa chừng vẫn bị từ chối như thường.
- **Không làm gì cho connection không qua tunnel.** Ở đó pool tự mở lại socket được rồi; không có
  gì hỏng để sửa.
- **Không giữ trạng thái phiên DB qua một lần mở lại.** Bảng tạm, `USE`, biến session, transaction
  đang mở — tất cả thuộc về kết nối cũ và mất theo nó. Xem mục Rủi ro.
- **Không có test tự động cho đường đi thật của tunnel.** Cần một máy chủ SSH sống; kiểm bằng tay.

## Hiện trạng

Những gì đọc được trong code, và chỗ nào thật sự hỏng:

| Chỗ | Sự thật |
| --- | --- |
| [`ssh/mod.rs:163`](../../apps/desktop/src-tauri/src/ssh/mod.rs#L163) | `client::Config` chỉ đặt `nodelay` và `window_size`. russh mặc định `keepalive_interval: None` — không gói nào giữ phiên, và không gì phát hiện phiên đã chết |
| [`ssh/mod.rs:248`](../../apps/desktop/src-tauri/src/ssh/mod.rs#L248) | `open_tunnel` xác thực **một lần**, gói `Handle` vào `Arc` và chia cho mọi kết nối bridge. Phiên chết là handle chết vĩnh viễn |
| [`ssh/mod.rs:289`](../../apps/desktop/src-tauri/src/ssh/mod.rs#L289) | `bridge_connection` gặp `channel_open_direct_tcpip` lỗi thì `return` — socket local đóng ngay, driver đọc được EOF |
| vòng `accept` | Vẫn chạy, cổng local vẫn mở. Nên mọi kết nối pool mở mới đều thất bại y hệt, mãi mãi. Đây chính là "xem các bảng thì không thấy data nữa, giống như đã mất kết nối" |
| sqlx 0.9 `pool/options.rs:149-162` | Mặc định đã là `test_before_acquire: true`, `idle_timeout` 10 phút, `max_lifetime` 30 phút, `acquire_timeout` 30 giây |
| sqlx 0.9 `net/socket/buffered.rs:290` | Câu "expected to read N bytes, got 0 bytes at EOF" là `io::ErrorKind::UnexpectedEof`, tới `AppError` qua `sqlx::Error::Io` |
| [`drivers/redis.rs:60`](../../apps/desktop/src-tauri/src/modules/db/drivers/redis.rs#L60) | Redis đi qua `ConnectionManager`, tự quay số lại |
| [`drivers/mongo.rs`](../../apps/desktop/src-tauri/src/modules/db/drivers/mongo.rs) | Mongo `Client` tự dựng lại topology, và mặc định `retryReads` là bật |

Kết luận rút ra từ bảng trên, và là điều làm cho công việc này nhỏ hơn vẻ ngoài của nó: **chỉ có
đúng một chỗ hỏng thật — phiên SSH nằm sau listener.** Cả bốn driver đều đã biết tự quay số lại;
điều duy nhất chúng không làm được là dựng lại một đường ống mà chúng không biết là có tồn tại.

## Quyết định nền: giữ nguyên cổng local

`TcpListener` và cổng nó bind (`ssh/mod.rs:256`) **không bao giờ bị thay**. Chỉ phiên SSH phía sau
được mở lại.

Vì sao đây là quyết định đáng đặt lên đầu: cổng local là thứ duy nhất mà `ActiveConnection.endpoint`,
mọi pool đang mở, mọi client và cả hai công cụ dump/restore đã ghi nhớ. Giữ nó nguyên nghĩa là
[`state.rs`](../../apps/desktop/src-tauri/src/modules/db/state.rs), `connect_db` và toàn bộ các file lệnh
không cần đổi một dòng nào cho việc hồi phục — driver thấy "socket bị từ chối" rồi "socket lại chấp
nhận", đúng thứ chúng đã biết xử lý.

## 1. Keepalive

Trong `authenticate_inner`:

```rust
let config = Arc::new(client::Config {
    nodelay: true,
    window_size: WINDOW_SIZE,
    keepalive_interval: Some(KEEPALIVE_INTERVAL), // 15s
    keepalive_max: KEEPALIVE_MAX,                 // 3 — cũng là mặc định của russh
    ..client::Config::default()
});
```

15 giây: đủ ngắn để đi trước idle timeout của một NAT gia đình (thường 300 giây) và trước
`ClientAliveInterval` của sshd; đủ dài để một phiên để không cả ngày cũng chỉ tốn vài trăm byte.
Ba lần không trả lời (`russh/src/client/mod.rs:1236`) thì russh kết thúc phiên — nghĩa là đường
chết được phát hiện trong khoảng 45 giây, và `Handle::is_closed()` (`client/mod.rs:289`) trả `true`
từ lúc đó.

`inactivity_timeout` giữ nguyên `None`: nó đóng phiên khi không có traffic, đúng thứ đang muốn tránh.

## 2. Phiên tự mở lại

`Tunnel` không còn chỉ là một `JoinHandle`:

```rust
struct TunnelInner {
    ssh: SshConfig,
    remote_host: String,
    remote_port: u16,
    app_data: PathBuf,
    notify: TunnelNotify,
    /// Phiên đang dùng. `None` nghĩa là chưa có hoặc lần mở lại gần nhất thất bại.
    session: tokio::sync::Mutex<SessionSlot>,
}

struct SessionSlot {
    handle: Option<Arc<client::Handle<TunnelHandler>>>,
    /// Lần thất bại gần nhất, để không nện máy chủ SSH bằng một chuỗi xác thực hỏng.
    failed_at: Option<Instant>,
}

pub struct Tunnel {
    inner: Arc<TunnelInner>,
    accept: JoinHandle<()>,
    watch: JoinHandle<()>,
}
```

Mọi chỗ cần phiên đều đi qua một hàm:

```rust
impl TunnelInner {
    async fn session(&self) -> Result<Arc<client::Handle<TunnelHandler>>, AppError> {
        let mut slot = self.session.lock().await;
        if let Some(handle) = slot.handle.as_ref().filter(|h| !h.is_closed()) {
            return Ok(Arc::clone(handle));
        }
        if let Some(at) = slot.failed_at {
            if at.elapsed() < RETRY_COOLDOWN {           // 3s
                return Err(err!("error.sshUnavailable"));
            }
        }
        (self.notify)(TunnelEvent::Reconnecting);
        match authenticate(&self.ssh, &self.app_data).await {
            Ok(session) => {
                let handle = Arc::new(session);
                *slot = SessionSlot { handle: Some(Arc::clone(&handle)), failed_at: None };
                (self.notify)(TunnelEvent::Reconnected);
                Ok(handle)
            }
            Err(e) => {
                *slot = SessionSlot { handle: None, failed_at: Some(Instant::now()) };
                (self.notify)(TunnelEvent::Failed(e.clone()));
                Err(e)
            }
        }
    }
}
```

Ba điều đáng nói về hàm này:

**Khoá giữ suốt lần xác thực là cố ý.** Pool mở năm kết nối cùng lúc thì cả năm dừng lại sau một
lần `authenticate`, không phải năm lần. Cái giá là khi đang mở lại, mọi kết nối mới qua tunnel này
chờ tối đa `CONNECT_TIMEOUT` (10 giây) — nằm gọn trong `acquire_timeout` 30 giây của sqlx.

**`failed_at` là để bảo vệ máy chủ SSH.** Không có nó, một pool đang cố mở kết nối trong lúc mạng
chết sẽ bắn hàng chục lần xác thực mỗi phút vào một sshd có `MaxAuthTries` — và có thể có fail2ban.

**`is_closed()` là đường phát hiện nhanh, không phải đường duy nhất.** Nếu phiên chết theo kiểu
`is_closed()` chưa kịp thấy, `channel_open_direct_tcpip` sẽ lỗi; `bridge_connection` khi đó bỏ phiên
hiện tại (`slot.handle = None`) rồi gọi `session()` lại đúng một lần trước khi buông socket local.

`bridge_connection` do đó nhận `&Arc<TunnelInner>` thay vì `&Handle`, và vòng `accept` cũng chỉ giữ
`Arc<TunnelInner>`.

## 3. Watcher và sự kiện `tunnel://state`

Chỉ mở lại khi có ai đó gõ cửa thì banner chỉ hiện sau khi người dùng đã bấm vào một thứ và chờ.
Người dùng đã yêu cầu "lúc kết nối lại cần có thông báo lên cho user hiểu", nên tunnel tự canh:

```rust
// Trong open_tunnel, cạnh vòng accept.
let watch = tokio::spawn(async move {
    let mut wait = WATCH_IDLE;                 // 15s khi mọi thứ đang ổn
    loop {
        tokio::time::sleep(wait).await;
        let dead = { inner.session.lock().await.handle.as_ref().is_none_or(|h| h.is_closed()) };
        if !dead {
            wait = WATCH_IDLE;
            continue;
        }
        // Hỏng thì thử ngay, và giãn dần nếu vẫn hỏng: 5s, 10s, 20s… tới trần WATCH_MAX = 60s.
        wait = next_backoff(wait, inner.session().await.is_ok());
    }
});
```

Nghĩa là: máy tính ngủ dậy, đường mạng về, tab tự lành và banner tự chuyển sang *đã kết nối lại*
trước khi người dùng bấm bất cứ thứ gì. Còn khi máy chủ SSH thật sự không tới được, nhịp thử giãn
dần ra một phút một lần thay vì gõ liên tục.

Sự kiện đi lên frontend theo đúng lối `transfer://progress` đã có — `ssh/mod.rs` không được biết gì
về Tauri, nên nó nhận một callback:

```rust
pub enum TunnelEvent { Reconnecting, Reconnected, Failed(AppError) }
pub type TunnelNotify = Arc<dyn Fn(TunnelEvent) + Send + Sync>;
```

`commands/mod.rs` dựng closure đó y như `reporter(app, id)` (`commands/mod.rs:51`) và `app.emit` lên
`tunnel://state` với payload:

```ts
{ id: string; state: "reconnecting" | "reconnected" | "failed"; error?: AppError }
```

**Một thay đổi nhỏ nhưng bắt buộc trong `connect_db`:** id của connection hiện được sinh ở
[`commands/mod.rs:187`](../../apps/desktop/src-tauri/src/modules/db/commands/mod.rs#L187), tức là *sau*
`resolve_endpoint`. Closure cần id, nên `Uuid::new_v4()` chuyển lên đầu hàm và `resolve_endpoint`
nhận thêm tham số `notify`.

Nút *Thử lại* của banner gọi một lệnh mới `tunnel_reconnect(id)`: nó xoá `failed_at` rồi gọi
`session()`, để người dùng không phải chờ hết nhịp backoff. Đây là chỗ đầu tiên đọc tới
`ActiveConnection.tunnel`, nên `#[allow(dead_code)]` ở [`state.rs`](../../apps/desktop/src-tauri/src/modules/db/state.rs)
bỏ đi được.

## 4. Pool: vì sao không đổi gì

Phần "pool hardening" trong hướng đã chốt hoá ra gần như không có việc để làm, và ghi lại ở đây để
lần sau không ai đi làm lại:

| Thứ định đặt | Mặc định sqlx 0.9 | Kết luận |
| --- | --- | --- |
| `test_before_acquire` | `true` | Đã đúng. Đây là thứ khiến kết nối chết trong pool bị loại và mở lại — và mở lại là thứ đánh thức tunnel |
| `idle_timeout` | 10 phút | Đủ. Siết xuống 5 phút chỉ rút ngắn một cửa sổ mà `test_before_acquire` đã che |
| `max_lifetime` | 30 phút | Đúng bằng con số định đặt |
| `acquire_timeout` | 30 giây | Phải **lớn hơn** `CONNECT_TIMEOUT` 10 giây của một lần mở lại phiên. 30 thoả; 10 như dự tính ban đầu thì hụt |

Nên `MySqlPoolOptions`/`PgPoolOptions` giữ nguyên `max_connections(5)` và không thêm dòng nào.
Viết lại một mặc định thành chính nó chỉ tạo ra nhiễu và một con số nữa phải bảo trì.

## 5. Nhận diện mất kết nối và chạy lại lệnh đọc

Trong `drivers/mysql.rs` và `drivers/postgres.rs`:

```rust
/// Lỗi này là đường ống đứt, không phải máy chủ từ chối.
fn lost_connection(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Io(_) | sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed)
}

pub(super) fn map_error(e: sqlx::Error) -> AppError {
    if lost_connection(&e) { err!("error.connectionLost") } else { err!("error.mysql", message = e) }
}
```

Rồi thay 46 chỗ `err!("error.mysql", message = e)` và 44 chỗ `err!("error.postgres", message = e)`
thành `map_error` — máy móc, và chính vì máy móc nên là commit riêng, không trộn với gì khác. Cả 46
chỗ MySQL nằm gọn trong ba file driver (`mysql.rs` 32, `mysql_structure.rs` 10, `mysql_script.rs` 4)
và đều đúng dạng `message = e`; chỗ nào `e` hoá ra không phải `sqlx::Error` thì trình biên dịch nói
ngay, và chỗ đó giữ nguyên `err!` cũ.

`error.connectionLost` cố ý **không mang** `message`: nguyên văn của sqlx ở đây là chuyện nội bộ
của thư viện ("expected to read 4 bytes…"), không phải máy chủ nói, nên không thuộc diện được giữ
nguyên như quy ước ở [`error.rs`](../../apps/desktop/src-tauri/src/error.rs) mô tả.

Chạy lại lệnh đọc, trong `commands/mod.rs`:

```rust
macro_rules! retry_read {
    ($body:block) => {{
        match async $body.await {
            Err(e) if e.code == "error.connectionLost" => async $body.await,
            other => other,
        }
    }};
}
```

Macro chứ không phải hàm nhận closure: một `Fn() -> impl Future` mượn `State<'_, DbState>` đưa lời
gọi vào đúng loại rắc rối lifetime không đáng đánh nhau, còn macro thì chỉ là viết thân lệnh hai lần.

Không cần ngủ giữa hai lần: lần thứ hai `acquire` sẽ tự nằm chờ sau `session()` đang mở lại.

Áp cho **đúng các lệnh đọc**, mỗi engine tám cái: `list_databases`, `server_info`, `list_tables`,
`table_stats`, `table_data`, `table_structure`, `schema_outline`, `collations`.

Không áp cho: `query`, `run_script`, `update_row`, `insert_rows`, `delete_rows`, toàn bộ DDL,
`dump`/`restore` (chúng tự quay số lấy, không mượn pool), và `validate_sql` (chạy theo nhịp gõ,
một lần lỗi không đáng nhân đôi).

Mongo và Redis không cần gì thêm: driver của chúng đã tự thử lại lệnh đọc.

## 6. Giao diện

| File | Việc |
| --- | --- |
| `src/modules/db/tunnel.ts` | `TunnelState`, `onTunnelState(id, cb)` — bản sao đúng khuôn của `transfer.ts` (cả hai nay nằm trong [`db/api.ts`](../../apps/desktop/src/modules/db/api.ts), T208) |
| `src/modules/db/components/TunnelBanner/` | Component + CSS Module + `index.ts`, theo [component-structure](../standards/desktop/component-structure.md) |
| `src/modules/db/components/TunnelBanner/state.ts` | `nextBannerState(current, event)` — hàm thuần, chỗ duy nhất có gì để test |
| `sql/SqlWorkspace.tsx`, `mongo/MongoWorkspace.tsx`, `redis/RedisWorkspace.tsx` | Một dòng, ngay trên `<ErrorBanner>` sẵn có |
| `src/modules/db/i18n/{en,vi}.ts` | `tunnel.reconnecting`, `tunnel.reconnected`, `tunnel.failed`, `tunnel.retry`, `error.connectionLost`, `error.sshUnavailable` |

Banner đặt trong từng workspace chứ không phải trong `DbTab`: `DbTab` trả workspace ra làm gốc của
tab, bọc thêm một `div` sẽ phá layout flex của cả ba.

Ba trạng thái:

- **reconnecting** — nền cảnh báo, có spinner, không đóng được. "Mất kết nối SSH — đang kết nối lại…"
- **reconnected** — nền thành công, tự biến mất sau 3 giây. "Đã kết nối lại."
- **failed** — nền lỗi, ở lại tới khi có `reconnected`, kèm câu lỗi thật (khoá sai, host không tới
  được) và nút *Thử lại* gọi `tunnel_reconnect`.

Connection không có tunnel thì không có sự kiện nào, nên không có banner nào — không cần cờ riêng.

## 7. Kiểm thử

**`cargo test`** — phần thuần:

- `lost_connection` nhận đúng `Io`/`PoolTimedOut`/`PoolClosed` và **không** nhận `Database(_)`
  (lỗi cú pháp SQL không được biến thành "mất kết nối").
- `map_error` trả `error.connectionLost` cho `Io`, `error.mysql` cho phần còn lại.
- Nhịp của watcher tách thành hàm thuần `next_backoff(current, ok) -> Duration`, định nghĩa là:
  thành công về `WATCH_IDLE` (15 giây); thất bại lần đầu — tức `current` đang là nhịp nghỉ — xuống
  `WATCH_MIN` (5 giây); thất bại tiếp thì nhân đôi tới trần `WATCH_MAX` (60 giây). Test đúng ba
  nhánh đó, gồm cả việc chạm trần rồi ở lại đó.
- `retry_read!` với một bộ đếm: gọi hai lần khi lần đầu là `connectionLost`, một lần khi lần đầu
  thành công, một lần khi lỗi là lỗi khác.

**`npm test`** — `nextBannerState`: reconnecting→reconnected→ẩn, failed ở lại qua nhiều lần
`failed`, `reconnected` sau `failed` thì xoá lỗi.

**Bằng tay** (`npm run dev:app`) — không có cách nào khác cho đường đi thật:

1. Mở một connection MySQL qua SSH tunnel, xem được dữ liệu một bảng.
2. Trên máy chủ SSH, giết đúng phiên đó (`pkill -f "sshd: <user>"`), hoặc rút mạng máy client
   khoảng một phút.
3. Trong vòng ~45 giây banner phải tự hiện *đang kết nối lại* rồi *đã kết nối lại* — không cần bấm gì.
4. Bấm vào một bảng: dữ liệu về bình thường.
5. Lặp lại bước 2 nhưng chặn hẳn cổng SSH: banner ở trạng thái *thất bại*, nút *Thử lại* bấm được,
   và log không cho thấy một chuỗi xác thực dồn dập.
6. Làm lại toàn bộ với một tab Redis và một tab Mongo qua tunnel.

## 8. Thứ tự commit

1. `feat(ssh): keep the tunnel alive and reopen its session` — mục 1, 2, 3 phía backend, kèm test
   thuần cho backoff. Chưa có UI, nhưng tự nó đã sửa được lỗi: tab tự lành, chỉ là im lặng.
2. `feat(db): tell a lost connection from a server error` — mục 5 phần `map_error`, thay 90 chỗ.
3. `feat(db): retry a read after the tunnel comes back` — mục 5 phần `retry_read!`.
4. `feat(db): say when the SSH tunnel is being reopened` — mục 6, cộng `tunnel_reconnect`.

Mỗi commit một dòng CHANGELOG theo [quy ước](../standards/desktop/changelog.md); cả bốn nằm
dưới `### Added`/`### Changed` chứ không phải `### Fixed` — bản có lỗi này chưa phát hành.

## Rủi ro và đánh đổi

- **Mở lại đường ống không phải mở lại phiên DB.** Bảng tạm, `USE`, biến session và transaction
  đang mở đều thuộc kết nối cũ. Một script đang chạy dở khi tunnel đứt sẽ không chạy tiếp — đúng
  với quyết định "chỉ lệnh đọc", nhưng đáng nói ra: người dùng có thể tưởng "đã kết nối lại" nghĩa
  là mọi thứ trở lại y nguyên.
- **Lệnh đọc chạy lại có thể trả dữ liệu của thời điểm khác** vài giây sau. Với một bảng đang được
  ghi thì hai lần đọc cho hai kết quả. Chấp nhận: đó cũng là điều xảy ra khi người dùng tự bấm lại.
- **Một lần mở lại chậm làm chậm mọi thứ chung connection đó** — khoá phiên serialise hoá mọi lần
  mở channel trong tối đa 10 giây. Đây là cái giá đã chọn để đổi lấy "một lần xác thực cho cả pool".
- **Task bridge đang sống vẫn giữ `Arc<TunnelInner>`** sau khi `Tunnel` bị drop, nên phiên SSH chỉ
  thật sự đóng khi kết nối cuối cùng qua nó đóng. Đây là hành vi đã có từ trước, không phải cái mới.
- **Watcher chạy một task nữa cho mỗi tunnel.** Với vài tab thì không đáng kể; nếu sau này có
  chuyện mở hàng chục connection cùng lúc thì đây là chỗ đầu tiên phải xem lại.
