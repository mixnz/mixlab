---
status: implemented
date: 2026-09-07
---

# Metrics và Settings: phần còn lại của Pha 4, module `mixengine`

Ngày 2026-09-07. Phần cuối của Pha 4 trong [roadmap/mixengine-module.md](../roadmap/desktop/mixengine-module.md)
(T4.1, T4.2, T4.6–T4.8) — nối tiếp
[2026-09-06-mixengine-blueprints-extensions-design.md](2026-09-06-mixengine-blueprints-extensions-design.md)
(Blueprints/Extensions, đã xong ở `00eed71` #42). Spec đó để lại đúng ba câu hỏi ở mục "Nợ"; spec này
mở đầu bằng cách trả lời cả ba, rồi thiết kế trên câu trả lời đó.

**Cập nhật 2026-09-07 (sau khi gửi câu hỏi cho MixEngine):** bên MixEngine xác nhận lại và sửa một
chi tiết — T96 (`daemon.disk_usage`/`daemon.cleanup`) lên từ `v0.0.3` (PR
[#103](https://github.com/mixnz/mixengine/pull/103), commit `d82c033`), không phải `v0.0.4` như bản
đọc đầu ghi nhầm; `v0.0.4` chỉ sửa một lỗi bóc tarball không liên quan (xem "Hiện trạng"). Quyết định
D0 (vendor `v0.0.4`) không đổi. Phần "Nợ" mục 1 (default web server) được bổ sung nguyên hợp đồng
tương lai họ đã xác nhận, để lần build sau không phải tra cứu lại.

## Mục tiêu

- **Dashboard, mở rộng** (màn đã có từ Pha 1, không phải màn mới): CPU %/RSS theo từng service trong
  lúc tab đang mở, cộng bảng disk usage 5 hạng mục kèm nút dọn dẹp.
- **Metrics** — mục sidebar mới: lịch sử 24 giờ theo từng service (và daemon), trả lời đúng câu
  roadmap đặt ra: *"đêm qua cái gì ăn pin của tôi"*.
- **Settings** — bật mục sidebar đang để `disabled` làm chỗ giữ: root directory, TLD quản lý,
  autostart, updates, `daemon.doctor`/`doctor_repair`, gỡ MixEngine (`uninstall_plan` trước, luôn),
  diagnostics bundle.

## Phi mục tiêu

- **"Default web server"** (`service.set_front_end`, T97/ADR 0026) — thiết kế đã chốt phía MixEngine
  nhưng **chưa lên bản release nào**, xem "Hiện trạng" và "Nợ". Không hoãn cả màn Settings vì việc
  này — năm phần còn lại của Settings không phụ thuộc nó.
- Tự động hoá việc vendor bindings (một script trong `scripts/`) — vẫn là câu hỏi mở gốc của roadmap
  ("Vendor `bindings/` bằng cách nào"). Spec này chỉ **thực hiện một lần re-vendor thủ công** (D0),
  không thiết kế cơ chế chung.
- Tray/menu-bar — câu hỏi mở khác của roadmap, không liên quan tới Metrics/Settings.
- Sửa gì khác ở Dashboard ngoài phần CPU/RSS + disk usage thêm vào.

## Hiện trạng

### Phương pháp: đọc thẳng `master` và bản release mới nhất hôm nay, không tin cache

Bài học spec trước ("đọc `crates/mixengine-proto/src/rpc.rs` thật, không tin `.md`") áp tiếp, cộng
thêm một bước: `.md` phía MixEngine (`client-surface.md`, `daemon-and-ipc.md`) tự nó cũng đổi theo
thời gian — bản tải cho spec này (2026-09-07) đã sửa lại đúng ba chỗ spec trước gắn cờ "chưa có trả
lời". Đã tải và đọc trực tiếp từ `mixnz/mixengine`:

- `crates/mixengine-proto/src/rpc.rs` — bảng hằng số method, trên `master`.
- `crates/mixengine-proto/src/metrics.rs` — định nghĩa `MetricsSubject` thật, kèm `Display`/`parse`.
- `docs/features/client-surface.md`, `docs/architecture/daemon-and-ipc.md` — bản `master`.
- `docs/specs/2026-08-30-t71-metrics-history-design.md` (phía MixEngine) — thiết kế gốc
  của T71, để xác nhận việc chia màn Dashboard/Metrics là quyết định của client, không phải điều họ
  đã giả định sẵn.
- Tarball `mixengine-api-0.0.4-typescript.tar.gz` — bản release ký mới nhất (`v0.0.4`, phát hành
  2026-09-06T19:30:20Z, tức **một ngày sau** spec Blueprints/Extensions), so với
  `src/modules/mixengine/api/types/VERSION` đang vendor (`0.0.1-beta.1`).

### Ba câu hỏi "Nợ" của spec trước — cả ba đều đóng được

| # | Câu hỏi | Trả lời |
| --- | --- | --- |
| 1 | Dashboard khớp `MetricsSubject` với service bằng cách nào? | **Đã có công thức chính xác.** `MetricsSubject` lên dây là một chuỗi đóng: `"daemon"` hoặc `"service:<id>"` (`MetricsSubject::Display`/`parse`, `mixengine-proto/src/metrics.rs:67-90`). Khớp một `ServiceRow.id` là so `subject === \`service:${row.id}\``, không suy đoán gì. `client-surface.md` (bản 2026-09-06) tự sửa "in one read" thành hai lượt: `service.list` một lần, giữ `/metrics` stream mở, không gọi `metrics.snapshot` mỗi lần vẽ — gọi vậy là phá đúng bất biến T71 ("mẫu nhanh chỉ tồn tại lúc có ai đang xem"). |
| 2 | Disk usage 4 hạng mục + cleanup — API có không? | **Có, mới thêm (T96), lên từ `v0.0.3`** (PR [#103](https://github.com/mixnz/mixengine/pull/103), commit `d82c033`) — không phải `v0.0.4` như bản đọc đầu tiên của spec này ghi nhầm; `v0.0.4` chỉ sửa một lỗi bóc tarball không liên quan tới hai method này, xem "Hiện trạng". `daemon.disk_usage` (`DiskUsageQuery{refresh}` → `DiskUsage`) và `daemon.cleanup` (`CleanupQuery{keep_logs,keep_cache}` → job → `CleanupReport`). Roadmap/`client-surface.md` cũ nói "4 hạng mục", bản thật là **5**: runtimes, data, logs, certs, cache — và chỉ hai (logs, cache) dọn được qua `daemon.cleanup`. |
| 3 | "Default web server" đọc/ghi ở đâu? | **Trả lời được về thiết kế, chưa dùng được hôm nay.** `service.set_front_end` (T97, [ADR 0026](https://github.com/mixnz/mixengine/blob/master/docs/decisions/0026-the-active-front-end-is-a-row-and-switching-it-is-a-job.md)) là một **job**, không phải setting — vì grant cổng 80/443 trên Linux đổi theo binary nào cần nó. Đọc lại qua `ServiceSummary.role`, **không có method đọc riêng**. Nhưng cả `service.set_front_end` lẫn `ServiceSummary.role` **không có trong `v0.0.4`** — xác nhận bằng cách tải và giải nén chính tarball ký của bản release đó, không chỉ đọc `master`. Đây là code đã viết nhưng chưa phát hành; xem "Nợ". |

### Bindings đang vendor cũ hơn bản release mới nhất một bậc

`src/modules/mixengine/api/types/VERSION` = `0.0.1-beta.1`. Bản ký mới nhất trên GitHub Releases là
`v0.0.4`. So hai bộ (`diff -rq`, không sửa tay theo đúng luật roadmap):

- **12 file type mới**, ứng với đúng **ba method mới** kể từ `0.0.1-beta.1` — MixEngine xác nhận diff
  là thuần cộng thêm, không xoá method nào: `daemon.disk_usage`/`daemon.cleanup` (T96, phần spec này
  cần — `CategoryUsage`, `Cleaned`, `Cleanup`, `CleanupQuery`, `CleanupReport`, `DiskCategory`,
  `DiskUsage`, `DiskUsageQuery`, `Reclaim`) và `database.credentials` (T77b, `DatabaseCredentials`/
  `DatabaseCredentialsQuery` — phần thưởng đi kèm re-vendor, **ngoài phạm vi spec này**, để dành cho
  Sites/Projects/Database). `ExtensionAvailable` là type tham số của `extension.available` đã dùng ở
  Pha 4 Blueprints/Extensions, không phải method mới.
- **5 file đổi, cả năm đều cộng thêm field optional** (không phá gì Pha 2/3 đang dùng):
  `DatabaseCreate` (+`password?`), `PackageFilter`/`RuntimeFilter` (+`refresh?`), `index.ts`,
  `package.json`.
- **Nhóm `Metrics*` không đổi** giữa `0.0.1-beta.1` và `v0.0.4` — màn Metrics dựng được trên bộ vendor
  hiện tại, không cần chờ bước re-vendor dưới đây.
- **`FrontEndSwitch`/`FrontEndReport`/`ServiceSummary.role` vắng mặt ở cả hai** — xác nhận lại mục 3
  ở trên bằng một nguồn thứ hai độc lập với việc đọc `rpc.rs` trên `master`, và bằng chính lời xác
  nhận của MixEngine: code đã merge (`d6346a1`, PR [#106](https://github.com/mixnz/mixengine/pull/106))
  nhưng chưa có trong tag nào — `v0.0.4` chỉ có ADR 0026, chưa có hằng RPC hay type.

**`v0.0.3` so với `v0.0.4`.** Ba method mới (kể cả `daemon.disk_usage`/`daemon.cleanup`) đã có từ
`v0.0.3`. `v0.0.4` chỉ sửa một lỗi bóc tarball: bản `v0.0.3` đọc entry `./` (thứ `tar` luôn ghi ở đầu
mọi archive) như một đường dẫn thoát khỏi thư mục đích, khiến daemon bản đó **không cài được
runtime/package nào trên macOS và Linux** — một lỗi của chính `mixengined`, không phải của tarball
bindings TypeScript. Không liên quan trực tiếp tới việc vendor `api/types/`, nhưng là lý do luôn lấy
`v0.0.4` chứ không dừng ở `v0.0.3` cho bất cứ việc gì đụng tới bản MixEngine đang chạy.

**Quyết định D0 — re-vendor lên `v0.0.4` trước khi viết `commands.rs` cho phần disk usage/cleanup**,
tải đúng `mixengine-api-0.0.4-typescript.tar.gz` (asset đã ký cùng khoá với binary, tên đúng quy ước
roadmap mô tả), ghi `0.0.4` vào `VERSION`. Không re-vendor để đổi `service.set_front_end` — chưa có
bản nào mang nó; re-vendor lần nữa khi `v0.0.5` ra. Đây vẫn không phải câu trả lời chung cho "vendor
bằng cách nào" (script hay tay) — chỉ là một lần bump cụ thể; xem "Nợ".

## 1. Dashboard — mở rộng

### CPU %/RSS theo từng service

`/metrics` là một stream SSE riêng, **khác `/events`** mà Dashboard đã mở qua
[daemonWatch.ts](../../apps/desktop/src/modules/mixengine/daemonWatch.ts) — và khác theo đúng chiều ngược
nhau về vòng đời:

- **`/events` mở một lần cho cả đời app, không bao giờ đóng** ([daemonWatch.ts:12](../../apps/desktop/src/modules/mixengine/daemonWatch.ts)) — đúng, vì bus sự kiện tồn tại bất kể ai đang nhìn.
- **`/metrics` phải đóng khi Dashboard không còn là màn đang xem**, vì mở kết nối **chính là**
  subscribe: daemon lấy mẫu 1 Hz trong lúc có ai giữ stream, 1 lần/phút khi không — `client-surface.md`
  gọi thẳng đây là bất biến T71, và mở stream này theo khuôn `daemonWatch.ts` (mở một lần, không đóng)
  sẽ ép daemon lấy mẫu 1 Hz vĩnh viễn kể cả khi không ai nhìn Dashboard, đúng cái T71 sinh ra để tránh.

**Vì `MixEngineTab.tsx` giữ mọi màn đã-xem-qua ở trong DOM** (`mountedScreens`,
[MixEngineTab.tsx:53](../../apps/desktop/src/modules/mixengine/MixEngineTab.tsx)) **thay vì unmount lúc đổi
tab**, `useEffect` đóng/mở stream không thể khoá theo unmount — phải khoá theo prop `active` Dashboard
đã nhận sẵn: mở khi `active` chuyển `true`, đóng khi nó chuyển `false` hoặc component unmount, y hệt
cách `reload()` đã khoá theo `active` ở [Dashboard.tsx:115-117](../../apps/desktop/src/modules/mixengine/screens/Dashboard/Dashboard.tsx).

- **Backend**: một `MetricsState` thứ ba trong [state.rs](../../apps/desktop/src-tauri/src/modules/mixengine/state.rs),
  cùng hình dạng `keep`/`stop` với `LogsState` — không tái dùng `LogsState` cho việc này dù cùng là
  "một stream": `/events`, `/logs/{id}` và `/metrics` là ba kết nối có thể **cùng mở một lúc** cho
  cùng một tab (Dashboard giữ `/events` lẫn `/metrics`), nên gộp state là một stream giành khoá của
  stream kia. Một file `metrics.rs` mới, mirror gần như nguyên xi
  [logs.rs](../../apps/desktop/src-tauri/src/modules/mixengine/logs.rs) (`GET /metrics`, không tham số path,
  cùng `sse::Frames`).
- **Tauri command**: `mixengine_metrics_watch(onFrame: Channel<string>)` /
  `mixengine_metrics_unwatch()`, cùng khuôn `mixengine_logs_watch`/`unwatch`.
- **`api.ts`**: `metricsWatch(onFrame)` / `metricsUnwatch()`, cùng khuôn
  [`logsWatch`/`logsUnwatch`](../../apps/desktop/src/modules/mixengine/api.ts:311-323).
- **Vẽ**: một cột CPU % và một cột RSS thêm vào bảng Dashboard hiện có, khớp theo
  `subject === \`service:${row.id}\``. **Không có mẫu cho một service không nằm trong frame mới nhất
  là "chưa có số" (`—`), không phải `0%`** — đúng luật "một chỗ thiếu không suy ra bằng 0" áp cho cả
  `cpu_percent: null` trong một mẫu (T71 rule). Hàng `daemon` (subject `"daemon"`) không có `ServiceRow`
  tương ứng — vẽ thành một dòng tổng ở đầu hoặc chân bảng, không lẫn vào bảng service.
- **CPU/RSS là tổng theo process group** (php-fpm master + workers = một số) — đúng nguyên văn luật
  `client-surface.md` đã nêu, không phải điều Dashboard tự diễn giải.
- **Ba điểm MixEngine lưu ý khi trả lời câu hỏi #1 của spec trước, cả ba đều ảnh hưởng cách viết
  helper join:**
  - Prefix `service:` là *load-bearing*, không phải trang trí — `ServiceId::parse` chấp nhận tên
    trần, nên một service hoàn toàn có thể tên là `daemon`; dùng chung một spelling sẽ gán lịch sử
    của daemon cho service đó. Helper parse phía client phải giữ đúng prefix này, không rút gọn.
  - **Hàng `daemon` không bao gồm service nó supervise** (sửa ở T72) — hai không gian tách rời có
    chủ đích, nên nếu sau này Dashboard vẽ thêm một số "tổng", cộng thẳng các hàng lại là đúng, không
    double-count.
  - Type export phía TypeScript cố ý khai `String` trần (`ts(as = "String")`) — ngữ pháp
    `"daemon" | "service:<id>"` nằm trong `MetricsSubject::parse` phía Rust, TypeScript không kiểm
    chứng được (T56). **The standalone client phải tự viết helper parse/dựng chuỗi này phía client** (đúng
    `metricsSubjectFor` ở bảng Kiểm thử bên dưới) — không phải thứ suy ra được từ kiểu dữ liệu.

### Disk usage + cleanup

`daemon.disk_usage(DiskUsageQuery { refresh })` → `DiskUsage { root, measured_at, categories:
CategoryUsage[5], other_bytes }`. **Gọi với `refresh: false` lúc `reload()`** (đường Dashboard đã đọc
lại mỗi lần quay lại tab) — daemon tự giữ bản đọc một phút, đi bộ `runtimes/` là hàng chục nghìn file,
nên `refresh: false` là "đọc cái đã có", không phải "đọc cũ". Nút "Làm mới" riêng gửi `refresh: true`,
và `DiskUsage.measured_at` vẽ thành "đo lúc…" để không ai tưởng con số là tức thời.

Mỗi `CategoryUsage { id: DiskCategory, location, bytes, files, reclaim: Reclaim, unreadable? }` vẽ
khác nhau theo `reclaim`, **daemon nói cách dọn chứ không để client suy ra**:

| `Reclaim` | Hạng mục thật (đo được) | UI |
| --- | --- | --- |
| `by_cleanup { bytes, files }` | `logs`, `cache` | Checkbox trong dialog Cleanup (`keep_logs`/`keep_cache` — mặc định *bỏ tick* nghĩa là dọn), hiện đúng `bytes`/`files` API đã tính trước khi bấm |
| `by_method { method, because }` | `runtimes` | Không có nút dọn tại đây — hiện câu trỏ sang màn Runtimes (`runtime.uninstall`), đúng lý do `because` nêu (T32: không gỡ được runtime dưới một pool đang chạy) |
| `at_a_cost { because }` | `data` | Không có nút — hiện `because`, trỏ người dùng sang xoá site/database thủ công |
| `never { because }` | `certs` | Không có nút, hiện `because` |

`daemon.cleanup(CleanupQuery { keep_logs, keep_cache })` → job → `CleanupReport { items: Cleaned[2] }`
— **luôn đúng hai dòng** (`logs`, `cache`) dù kết quả là gì, theo đúng luật "một dòng trả lời *không
có gì ở đó* vẫn phải vẽ" T4.8 đã áp cho `daemon.bundle`. `Cleaned.outcome` (`Cleanup`) có 5 biến thể
(`empty`/`reclaimed`/`partial`/`kept`/`failed`) — `partial` mang cả `left_behind` lẫn `because`, vẽ
riêng chứ không gộp vào `reclaimed`. **Job này refuse nếu đang có `runtime.install`/`update.apply`
chạy** (`precondition_failed`, tên rõ lý do) — nút Cleanup không tự khoá theo trạng thái cục bộ, cứ
gửi và hiện đúng lỗi nếu bị từ chối.

## 2. Metrics — màn mới

**Chỉ lịch sử 24 giờ — không lặp lại số "bây giờ" Dashboard đã vẽ** (Quyết định D1). `metrics.history`
là một RPC đọc thường, không cần stream: `metrics.history(MetricsHistoryQuery { subject, since, until
})` → `MetricsHistory { minutes: MetricsMinute[], retention_hours }`.

- Bộ chọn subject: danh sách service từ `service.list` (đã có `api.services()`) cộng một mục cố định
  "daemon" (`MetricsSubject::Daemon`). Bỏ trống `subject` trong query là mọi subject cùng lúc — dùng
  cho một biểu đồ nhiều đường, không phải giá trị mặc định của bộ chọn.
- **Một phút không có dòng nào là một phút không ai đo, vẽ thành một khoảng trống trên biểu đồ, không
  nối hai điểm qua nó** — luật T71/`client-surface.md` áp thẳng vào đây, khác lỗi kinh điển "nối liền
  cho đẹp".
- Mỗi `MetricsMinute` mang cả `cpu_avg`/`cpu_peak` và `rss_avg`/`rss_peak`, cộng `samples` (1–60). Vẽ
  **peak** là câu trả lời cho "cái gì ăn pin của tôi" — trung bình một phút xoá mất một service ăn
  900 MB trong hai giây rồi thôi. `samples: 1` (không ai mở Dashboard/stream phút đó) vẽ nhạt hơn hoặc
  có chú thích khác `samples: 60`, không vẽ hai độ tin cậy như nhau.
- `retention_hours` vẽ thành một dòng "dữ liệu giữ trong N giờ" ở chân biểu đồ, giải thích vì sao
  trục thời gian dừng ở đó.
- **Không có thư viện chart nào trong `package.json` hôm nay.** Quyết định D4: dựng một SVG tối giản
  tay (đường polyline theo `cpu_avg`, dải mờ `cpu_peak`, trục thời gian rời rạc theo phút có dữ liệu),
  không thêm dependency mới cho một biểu đồ đường đơn giản.

## 3. Settings — màn mới

Bật mục Sidebar đang `disabled` ([Sidebar.tsx:31](../../apps/desktop/src/modules/mixengine/components/Sidebar/Sidebar.tsx)):
thêm `"settings"` vào `MixEngineScreen`/`SCREENS`
([tabState.ts:8-16](../../apps/desktop/src/modules/mixengine/tabState.ts)), một `pane("settings", …)` trong
[MixEngineTab.tsx](../../apps/desktop/src/modules/mixengine/MixEngineTab.tsx), gỡ `disabled`/`screen: null` của
mục đó trong `ITEMS`.

### Root directory, TLD quản lý — không cần call mới

Cả hai đọc từ `daemon.status()`, cuộc gọi Dashboard đã làm mỗi lần `reload()`: `home` (root) và
`dns?.wildcards` (TLD có wildcard) trên `DaemonStatus`
([DaemonStatus.ts](../../bindings/DaemonStatus.ts)). Settings tự gọi
`api.status()` riêng của nó (một request rẻ, không đáng chia sẻ state với Dashboard) — không cần
command Tauri mới, chỉ cần vẽ hai field này ra, thứ Dashboard hôm nay đọc mà không vẽ.

### Autostart

Ba command mới: `autostart.status` (không tham số) → `AutostartReport { mechanism, location, enabled,
changed, command?, for_this_home }`; `autostart.enable`/`autostart.disable`, cùng answer shape. Công
tắc đọc `enabled`, nhưng **phải phân biệt "bật" với "bật, cho một home khác"** qua `for_this_home` —
`enabled: true, for_this_home: false` vẽ thành công tắc bật nhưng có chú thích, không phải công tắc
tắt (T85b, đúng câu roadmap T4.6 đã ghi trước khi có method thật). `mechanism: "none"` vô hiệu hoá cả
công tắc, hiện `location` như một gợi ý lệnh chạy tay (`hint`-style, không phải lỗi).

### Updates

Bốn command đã có type vendor sẵn (`UpdateStatus`, `UpdateCheck`, `UpdateDecide`, `UpdateApplied`
đều đã nằm trong `api/types/` từ trước, không nằm trong phần cần re-vendor): `update.status` (đọc
rẻ, không ra mạng) hiện version mới nhất đã biết; nút "Kiểm tra ngay" gọi `update.check` (ra mạng,
lỗi mạng không phải error — `UpdateStatus.stale` báo thay); `update.decide` cho "bỏ qua bản này"/"nhắc
lại sau"; `update.apply` cài rồi **daemon tự thoát ngay sau khi trả lời** — cùng luật `daemon.shutdown`
đã có từ Pha 1 (T1.5: daemon không trả lời là một trạng thái đọc được, không phải lỗi). Refuse với
`precondition_failed` nếu bản cài là qua package manager — hiện đúng câu đó, không có nút retry.

### `daemon.doctor` / `daemon.doctor_repair`

`daemon.doctor()` (không tham số, đọc thuần — không ghi gì, không thể bật elevation) →
`DoctorReport { checks: Check[] }`, **vẽ đủ mọi check theo đúng thứ tự cố định kể cả `outcome: "ok"`**
— danh sách ngắn hơn trên một OS khác đọc như "sạch" thay vì "không hỏi câu đó ở đây", đúng luật type
đã ghi. `Outcome` bốn biến thể (`ok`/`note`/`problem`/`skipped`) vẽ bốn kiểu dòng khác nhau,
`problem.id: ProblemId` là khoá `daemon.doctor_repair` dùng để sửa.

`daemon.doctor_repair(DoctorRepair { grant })` — **hàng đợi elevation chung, không phải một dialog
riêng.** Doc-comment của `DoctorRepair.grant` nói thẳng: đường thường là hai lượt — gọi với
`grant: false` để enqueue, rồi `elevation.grant` sửa thật. Đây **chính là** hàng đợi
`elevation.status`/`ElevationDialog` Dashboard đã dựng ở Pha 1
([ElevationDialog](../../apps/desktop/src/modules/mixengine/components/ElevationDialog/ElevationDialog.tsx),
[pendingOps.ts](../../apps/desktop/src/modules/mixengine/pendingOps.ts)) — Settings gọi `doctor_repair` xong thì
mở lại đúng dialog đó, không viết luồng elevation thứ hai cho riêng Settings (Quyết định D3).

### Gỡ MixEngine

`daemon.uninstall_plan(UninstallQuery { keep_home: false, grant: false })` **trước và luôn luôn** — đọc
thuần (T87 rule, giống `daemon.doctor`) → `UninstallReport { items: Residue[] }`, **vẽ đủ 11+ dòng cố
định kể cả dòng trả lời "không có gì ở đây"** — cùng luật `daemon.doctor`/`daemon.bundle` đã lặp lại
ba lần trong roadmap giờ áp lần thứ tư. Checkbox `keep_home` đổi lại plan (gọi lại `uninstall_plan`
với giá trị mới) trước khi cho bấm nút Gỡ thật — không suy luận phần nào đổi khi tick, để daemon tự
nói.

Bấm Gỡ → mở `UninstallConfirmDialog` (T89 — cập nhật sau khi build): nút "Gỡ MixEngine" không gọi
`daemon.uninstall` thẳng nữa, nó chỉ mở dialog; `daemon.uninstall(UninstallQuery { keep_home, grant:
true })` chỉ chạy khi người dùng bấm "Gỡ" **trong dialog đó**. Lý do đổi so với thiết kế ban đầu ở
trên: `daemon.uninstall` vẫn là một job **tự nó bật đúng một prompt** (không qua `elevation.status`
như `doctor_repair` — field `grant` ở đây gộp thẳng vào job, `UninstallReport` không có field
`granting` vì lý do đó, đọc rõ trong doc-comment của `UninstallReport`), nhưng để cú click cuối cùng
trước khi hệ điều hành hỏi mật khẩu luôn là một cú click có chủ đích trong app — không phải "click lần
hai trên cùng một nút đổi tên" như bản đầu (dễ nhầm với một nút bị đơ, cùng lớp lỗi UAC-bật-sớm CA đã
gặp trước `mixengine_ca_repair`'s fix). `UninstallConfirmDialog` không đọc lại gì từ daemon — nó
không có `PendingOp[]` nào để vẽ như `ElevationDialog`, bảng residue phía sau nó đã là phần "xem
trước", dialog chỉ còn việc chặn cú click.

Khi `keep_home: false`, **daemon tự thoát sau khi job ghi xong kết quả**. `pollJob` không tự xác
minh lại bằng cách hỏi `presence` — `keep_home` là cờ chính request đã gửi, nên biết chắc daemon có
tự thoát hay không mà không cần đoán qua trạng thái máy đang chạy (một bản trước có tự chờ `presence`
rời `"running"` tối đa 10 lần trước khi quyết định, nhưng nếu nó không đổi kịp trong 10 giây đó thì
kẹt luôn ở Settings dù daemon *sẽ* rời). Khi `keep_home: false`, `pollJob` gọi thẳng `onUninstalled()`
— cùng cơ chế `onApplied` của `UpdatesSection`/`update.apply`
([MixEngineTab.pollUntilDaemonLeaves](../../apps/desktop/src/modules/mixengine/MixEngineTab.tsx)) — ngay khi
job báo `state` khác `"running"`, không dựa vào job `state: "succeeded"` một mình vì đó là thời điểm
daemon *sắp* thoát chứ chưa chắc đã thoát; `pollUntilDaemonLeaves` ở `MixEngineTab` mới là nơi chờ
`presence !== "running"` thật sự rồi để gate trên cùng tự vẽ đúng màn. `pollJob` cũng phải bắt lỗi RPC
của chính `jobStatus` trong đúng khoảnh khắc daemon thoát — kết nối rơi giữa chừng dễ bị transport báo
thành exception, và khi `keep_home: false` đó là dấu hiệu daemon đã rời (gọi `onUninstalled()` ngay)
chứ không phải một lỗi thật; chỉ khi `keep_home: true` (nơi daemon không có lý do gì để mất kết nối)
mới coi đó là lỗi thật và báo `onError`. Không bắt lỗi đó thì polling chết lặng lẽ và màn hình kẹt mãi
ở "Đang gỡ…" (cùng lớp lỗi 53387ea đã fix cho `update.apply`, dù cơ chế cụ thể khác — 53387ea không
có gì poll cả, ở đây có poll nhưng chết ở lần lỗi đầu tiên).

### Diagnostics bundle

`daemon.bundle(DiagnosticsBundle {})` (đối tượng rỗng, không tham số thật) → `BundleReport { path,
bytes, taken_at, members: Member[], omitted: Omission[] }`. Một nút "Xuất diagnostics", hiện `path`
kèm nút mở thư mục chứa — tái dùng đúng khả năng mở file manager the standalone client đã có sẵn cho Sites (Pha 2,
roadmap: "the standalone client đã có sẵn cả ba đường đó"), không viết lại. `omitted` vẽ thành một dòng phụ nói rõ
phần nào không nằm trong archive, không trình bày archive như đã đầy đủ (đúng câu T4.8 đã ghi).

### Default web server — chưa vẽ được, chỗ để dành

Một hàng thứ tư cạnh root/TLD/autostart, tắt/disable với chú thích "chờ bản MixEngine kế tiếp" thay vì
ẩn hẳn — cùng lý do `Sidebar.tsx` để Settings xám thay vì biến mất: một mục vô hình không ai biết nó
sắp tới. Khi `service.set_front_end`/`ServiceSummary.role` lên một bản release, đọc lại không tốn gì
mới — `ServiceSummary` là thứ Dashboard **đã** gọi qua `service.list`, chỉ thêm một field vào đúng chỗ
đang đọc; chỉ có phần ghi (`service.set_front_end`, một job) là việc thật sự mới lúc đó.

## Kiểm thử

Phần thuần, không cần daemon:

| Test | Nội dung |
| --- | --- |
| `metricsSubjectFor` | `service:<id>` dựng đúng từ một `ServiceRow.id`; `"daemon"` không khớp bất kỳ row nào |
| `metricsGapDrawing` | Một phút vắng trong `MetricsHistory.minutes` vẽ thành khoảng trống, không nối điểm; `cpu_avg: null` không vẽ như 0 |
| `diskCategoryPresentation` | Bốn biến thể `Reclaim` map đúng bốn kiểu dòng; `by_cleanup` là hai hạng mục duy nhất có checkbox |
| `cleanupReportRows` | `CleanupReport.items` luôn hai dòng (`logs`, `cache`) bất kể input; `partial` vẽ khác `reclaimed`/`kept` |
| `autostartSwitchLabel` | `enabled && !for_this_home` vẽ khác `enabled && for_this_home`; `mechanism: "none"` khoá công tắc |
| `doctorCheckOrder` | Danh sách `checks` giữ nguyên thứ tự và độ dài kể cả khi mọi `outcome` là `"ok"` — không lọc |
| `uninstallResidueRows` | `UninstallReport.items` vẽ đủ dòng kể cả `outcome` "không có gì ở đây"; đổi `keep_home` gọi lại `uninstall_plan`, không tự suy luận dòng nào đổi |
| Vòng đời `/metrics` theo `active` | Mount Dashboard với `active: false` không mở stream; `active` chuyển `true` mở, chuyển `false` đóng — test qua `MetricsState`-tương-đương phía test (mock transport, đếm số lần `keep`/`stop`) |

**Không test được bằng vitest, cần MixEngine thật:**

- `daemon.cleanup` có thật sự trả `precondition_failed` khi đang chạy `runtime.install`/`update.apply`
  song song không, và message đó đọc được tới đâu qua `errorMessage`.
- `DiskUsageQuery.refresh: false` trả một bản đọc bao cũ trong thực tế — tài liệu nói "giữ một phút"
  nhưng chưa đo trên daemon thật khoảng đó có đúng một phút hay là "tới lần đọc kế tiếp bất kể cách
  bao lâu".
- `update.apply` đóng kết nối theo đúng cách `daemon.shutdown`/`daemon.uninstall` đã đóng (client
  transport layer coi ba cái này như nhau chưa, hay `update.apply` có một đường lỗi riêng chưa test).
- `daemon.doctor_repair` với `grant: false` có luôn đi vào đúng hàng đợi `elevation.status` hay có
  trường hợp tự chạy thẳng không cần grant (ví dụ sửa nằm trong `MIXENGINE_HOME`, theo đúng câu
  `DAEMON_DOCTOR_REPAIR`'s doc-comment "một repair nằm trong `MIXENGINE_HOME` không xin quyền gì cả")
  — client phải xử lý cả nhánh "sửa xong luôn, không có gì để hiện dialog".

## Rủi ro

- **`MetricsState` thứ ba dễ bị nhầm là dư thừa và gộp vào `LogsState`** vì cả hai "cũng chỉ là một
  stream". Gộp sẽ làm Dashboard mở `/metrics` giành khoá của một `/logs/{id}` đang mở ở màn Logs (hai
  màn giữ mount cùng lúc theo kiến trúc `mountedScreens`) — đặt tên và một dòng comment ở đầu
  `metrics.rs` nói rõ lý do tách, như `state.rs` đã làm cho `LogsState`/`MixEngineState`.
- **Đóng stream `/metrics` theo `active` mà quên trường hợp component unmount thẳng** (đóng tab, tắt
  the standalone client) — `useEffect` cleanup phải chạy ở cả hai đường, không chỉ ở nhánh `active` đổi giá trị.
  Quên một trong hai là daemon kẹt ở lấy mẫu 1 Hz vĩnh viễn dù không ai còn xem — đúng bất biến T71
  spec này vừa dựa vào để thiết kế, vi phạm ngay trong lúc build.
- **Re-vendor lên `0.0.4` đổi `DatabaseCreate`/`PackageFilter`/`RuntimeFilter`** (thêm field optional)
  — không phá gì đang chạy vì cả ba đều thêm field mới, nhưng phải chạy lại **toàn bộ** test suite
  của Pha 2/3 sau khi re-vendor, không chỉ test phần Settings mới viết — một field mới trên một type
  đã dùng ở nơi khác vẫn là một thay đổi trên bề mặt build.
- **`update.apply` và `daemon.uninstall` (`keep_home: false`) đều kết thúc chính daemon đang phục vụ
  request đó.** Nếu transport layer coi "kết nối đóng giữa chừng" là lỗi mặc định (khác đường
  `daemon.shutdown` T1.5 đã xử đúng), cả hai màn sẽ hiện một `ErrorBanner` đỏ cho một thao tác vừa
  thành công — cần đi qua đúng nhánh "daemon chết là trạng thái đọc được" `presence` gate đã có, không
  viết một đường xử lý lỗi mới cho riêng hai method này.
- **`daemon.uninstall_plan` gọi lại mỗi lần đổi `keep_home`** có thể tạo cảm giác "chậm" nếu plan này
  cũng phải đi bộ đĩa như `disk_usage` — chưa đo trên daemon thật chi phí thật của
  `uninstall_plan` có nặng ngang `disk_usage(refresh: true)` hay rẻ hơn nhiều; nếu nặng, cần debounce
  thao tác tick checkbox trước khi gọi lại.

## Quyết định

**D0 — Re-vendor `api/types/` lên `v0.0.4`** trước khi viết `commands.rs` cho disk usage/cleanup. Bảy
file mới liên quan Settings, không đổi gì phá Pha 2/3. Không vendor phần `service.set_front_end` —
chưa có bản nào mang nó; không hand-type ba field thiếu, đúng luật roadmap "types là thứ đi mượn,
không phải thứ tự viết".

**D1 — Metrics là màn chỉ-lịch-sử.** Số CPU/RSS "bây giờ" vẽ trên Dashboard (nơi bảng service đã
đứng), không lặp lại trên Metrics. Lý do: vòng đời `/metrics` chỉ cần quản lý ở một chỗ, và nhân đôi
một component "giữ stream mở" ở hai màn là hai chỗ có thể quên đóng thay vì một.

**D2 — `/metrics` khoá vòng đời theo prop `active` của Dashboard, một `MetricsState` riêng mirror
`LogsState`** — không theo khuôn singleton-mở-suốt-đời `daemonWatch.ts` dùng cho `/events`. Hai stream
có mô hình chi phí ngược nhau: một cái rẻ để giữ mở mãi (bus sự kiện tồn tại sẵn), một cái đắt nếu
giữ mở mãi (ép daemon lấy mẫu nhanh vĩnh viễn) — copy khuôn sai chỗ là bug, không phải phong cách.

**D3 — `doctor_repair` dùng lại nguyên `ElevationDialog`/`pendingOps.ts` đã có từ Pha 1.** Không viết
dialog elevation thứ hai cho Settings: `DoctorRepair.grant: false` enqueue vào đúng hàng đợi chung
`elevation.status` đọc, `elevation.grant`/`elevation.drop` vẫn là hai lối ra duy nhất.

**D4 — Biểu đồ lịch sử Metrics là SVG dựng tay, không thêm thư viện chart.** `package.json` hôm nay
không có thư viện nào; một đường polyline + dải mờ cho peak là đủ cho yêu cầu, thêm dependency cho
việc này là đổi bề mặt build cho một thứ tự viết được trong một file.

**D5 — Settings là một mục sidebar riêng** (bật đúng chỗ `Sidebar.tsx` đã để `disabled`), không gộp
vào Dashboard: sáu mảng nội dung của nó (root/TLD, autostart, updates, doctor, uninstall, bundle)
không chia sẻ hình dạng RPC hay luồng tương tác với bảng service — khác trường hợp Runtimes+Packages
Pha 3 gộp vì cùng namespace.

## Nợ

Một câu hỏi thật còn treo, và hai câu hỏi cũ của roadmap spec này không đóng lại:

1. **"Default web server"** (T97) — **đã đóng 2026-09-08**: `v0.0.6` phát hành kèm
   `service.set_front_end`/`ServiceSummary.role`, bindings đã re-vendor lên `0.0.6` bằng
   `npm run bindings`, và `FrontEndSection.tsx` nối hàng này theo đúng hợp đồng dưới đây. Ghi chú gốc
   giữ lại làm bối cảnh — thiết kế đã có, code đã merge vào `master`
   (commit `d6346a1`, PR [#106](https://github.com/mixnz/mixengine/pull/106)), nhưng **chưa lên một
   bản release ký nào** tính đến hôm nay (`v0.0.4` mới chỉ có ADR 0026, chưa có hằng RPC/type). Không
   hand-type ba kiểu thiếu để né việc chờ — xem D0. MixEngine đã xác nhận nguyên hợp đồng cho lần
   build sau, chép lại đây để khỏi tra cứu lại khi `v0.0.5` ra:

   - **Đọc**: `ServiceSummary.role: Option<ServiceRole>` (internally tagged bằng field `role`), hai
     nhánh `front_end { server }` (server đang active, chính là giá trị `FrontEndServer` —
     `"caddy" | "nginx"`) và `other`. **`None` nghĩa là daemon build trước khi member này tồn tại**
     (ADR 0019, cùng luật `DaemonStatus.elevation`/`dns`/`update` đã theo) — **không phải** "chưa xác
     định được role". Một service không có recipe front-end là `other`, tức đã có câu trả lời, không
     phải một chỗ trống.
   - **Ghi**: `service.set_front_end` — một job (dừng server cũ, khởi động server mới), không phải
     ghi vào một setting. Không có method đọc riêng: đọc lại đúng `ServiceSummary.role` từ
     `service.list` — cuộc gọi Dashboard **đã** làm mỗi lần `reload()`.
   - Giá trị `server` đọc từ hàng đang active chính là giá trị `set_front_end` nhận vào — **The standalone client
     không cần tự map tên package sang một ý nghĩa** ở phía client.

   Hành động: kiểm lại trang Releases của `mixnz/mixengine` trước khi bắt tay dựng đúng một hàng này
   trong Settings; năm phần còn lại không chờ nó.
2. **Vendor bindings bằng cách nào** — câu hỏi mở gốc của roadmap, spec này chỉ làm một lần bump tay
   (D0), không thiết kế script `scripts/` tự động hoá việc này cho các lần sau.
3. **Không có bước xác minh chữ ký `.minisig`** trước khi vendor — đã tìm khắp repo
   (`grep -rl minisig`) và không thấy chỗ nào the standalone client xác minh chữ ký MixEngine trước khi tin nội dung
   tải về, kể cả cho tarball `bindings` lẫn binary. D0 tải và giải nén trực tiếp, cùng mức tin cậy
   spec trước đã ngầm chấp nhận — một bước xác minh thật (khoá công khai của MixEngine ở đâu, verify
   bằng gì) vẫn là việc chưa ai thiết kế.
