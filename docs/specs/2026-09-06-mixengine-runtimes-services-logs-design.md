---
status: implemented
date: 2026-09-06
---

# Projects, Runtimes & Packages, Services chi tiết và Logs: bốn màn hình mới của module `mixengine`

Ngày 2026-09-06. Pha 3 của [roadmap/mixengine-module.md](../roadmap/desktop/mixengine-module.md) — mở
rộng hơn ba màn hình roadmap gốc liệt kê, xem mục "Projects" và "Packages" dưới đây vì sao.

**Roadmap ghi "Pha 2–4 chưa bắt đầu", nhưng Pha 2 đã land** — `e7e1189`,
`feat(mixengine): sites, domains and TLS (#40)` — cùng Pha 1 (`898f936`). Đúng bài học Pha 0 đã tự ghi:
*"Code là câu trả lời, note không phải."* Header của roadmap nên sửa lại một dòng khi spec này được
chấp nhận; không phải việc của spec này.

## Mục tiêu

- **Projects**: màn hình mới, ngoài 9 màn hình gốc của `client-surface.md` — xem "Vì sao có Projects"
  dưới đây. Danh sách, tạo, sửa (tên/root/pin phiên bản/`keep_warm`), xoá.
- **Runtimes**: phiên bản PHP/Node/Python/Ruby đã cài (mặc định được đánh dấu), phiên bản có thể cài;
  cài/gỡ là job có tiến độ thật, dùng lại `applyJob`/`JobRow` (`daemonState.ts`) đã có từ trước — xem
  mục 3 cho lý do đây không phải hạ tầng mới.
  Bật/tắt PHP extension theo từng phiên bản — có method ghi thật, xem mục 2.
- **Packages** (MariaDB, Caddy, Redis…, khác `runtime.*`): cùng màn hình với Runtimes, hai tab con,
  cùng luồng cài/gỡ có tiến độ — theo đúng ý bạn: "package cần một luồng cài đặt y hệt Runtimes".
- **Services chi tiết**: mở từ một hàng ở Dashboard, xem giới hạn CPU/RAM (và watchdog bộ nhớ nơi máy
  không tự ép được), đổi ngưỡng tự dừng khi rảnh (idle), và với một service database: tạo
  database/account, và "Open" mở thẳng một tab `db` trong MixDB — không vòng qua OS.
- **Logs**: tail sống theo từng service, lọc theo stream, lộ đường file để mở thư mục chứa.

## Vì sao có Projects trong Pha này

Pha 2 chủ động không dựng Projects — quyết định (a) trong
[2026-09-06-mixengine-sites-domains-design.md](2026-09-06-mixengine-sites-domains-design.md), Câu hỏi
1: dropdown gọi thẳng `project.list`, máy chưa có project nào thì tự đăng ký bằng
`mix project add <thư mục>`. Bàn lại trong buổi viết spec này, quyết định đảo ngược: MixDB dựng hẳn
một màn hình quản lý — không chỉ vì Sites cần nó, mà vì `project.*` đã có đủ method
(`list, create, show, update, delete, export` — tên thật xác nhận qua `rpc.rs`, xem Hiện trạng) để một
màn hình đầy đủ không phải chắp vá. Đây **là**
một mục ngoài 9 màn hình `client-surface.md` liệt kê — xem **Quyết định D4** cho việc sidebar xử lý
thế nào.

## Phi mục tiêu

- Blueprints, Extensions (nghĩa "extension sản phẩm" — Mailpit, phpMyAdmin, chính MixDB — namespace
  `extension.*`), Settings, Metrics — Pha 4.
- **Tạo/xoá một service instance mới** (`service.create`/`service.delete` — cả hai đều tồn tại thật,
  xác nhận qua `rpc.rs`). Cài một package (mục 2) chỉ là "phiên bản này đã có trên đĩa" — dùng nó để
  dựng một service instance mới (`mariadb@secondary` cạnh `mariadb@main`) là một bước khác, vẫn ở
  ngoài Pha này: không nằm trong "Xong khi" của roadmap, và là một quyết định mở rộng phạm vi giống hệt
  Projects — nếu muốn thêm, nên hỏi trước như Projects đã được hỏi, không lặng lẽ gộp vào vì tiện.
  `mix service create` vẫn là đường duy nhất cho việc này ở Pha 3.
- Sửa `port`, `bind_addr`, `data_dir`, `autostart` của một service **đã tồn tại**. Roadmap T3.2 và
  "Xong khi" (*"sửa được port của MariaDB"*) giả định việc này làm được — **xác nhận là không**, và
  không phải vì chưa code tới: không có `service.config_get`/`config_set` nào cả, theo đúng chủ đích
  kiến trúc của MixEngine ("config sinh ra là đồ dùng một lần, không phải thứ có API đọc-ghi chung
  chung"). Roadmap nên bỏ câu "sửa được port của MariaDB" khỏi "Xong khi" của Pha 3 — đây không phải
  việc dời sang sau, mà là việc không tồn tại để làm. Xem **Quyết định D1**.

## Hiện trạng

### Ba nguồn, ba câu trả lời khác nhau — không nguồn nào một mình đủ tin

Viết spec này bắt đầu bằng đọc ba thứ: `bindings/` đã vendor trong repo này
(`src/modules/mixengine/api/types/`), `daemon-and-ipc.md` và `client-surface.md` đọc trực tiếp từ
`mixnz/mixengine@master`. Cả ba **không khớp nhau**, theo cả hai chiều:

| Method | Trong `bindings/` đã vendor | Trong `daemon-and-ipc.md` hôm nay | Trong `rpc.rs` thật |
| --- | --- | --- | --- |
| `service.config_get` / `config_set` | Không có type nào | Có, trong dòng `service.*` | **Không tồn tại** — xác nhận |
| `service.reload` | — | Có, trong dòng `service.*` | **Không tồn tại** — xác nhận |
| `service.create` / `delete` / `limits` / `set_limits` / `idle` / `set_idle` | Có type cho create/delete/limits/set_limits/set_idle | Không có trong dòng `service.*` | **Có cả sáu** — xác nhận |
| `package.*` (mariadb/caddy/redis, khác `runtime.*`) | `PackageCatalogue/Filter/Target/Release/Summary/Removal` | Không có namespace `package.*` nào | (chưa tra, giả định có theo bindings) |
| `runtime.list_extensions` / `set_extension` | `ExtensionChoice`, `ExtensionChange`, `PoolOutcome` | Không nêu tên hai verb này trong dòng `runtime.*` (chỉ 6 verb) | **Có** — xác nhận |
| `project.set_runtime` | — | Có, trong dòng `project.*` | **Không tồn tại** — tên thật là `project.update` |
| `project.get` | — | Có, trong dòng `project.*` | Tên thật là `project.show` |
| `project.import` | — | Có, trong dòng `project.*` | **Không tồn tại riêng** — `project.create` tự đảm nhận cả hai |
| `project.export` | — | Không có trong dòng `project.*` | **Có** |

**Kết luận: `daemon-and-ipc.md` lệch code thật (`rpc.rs`) ở ít nhất ba namespace** (`runtime.*`,
`service.*`, `project.*`), không phải một lần lẻ. Namespace nào Pha này còn phải tra thêm mà chưa đối
chiếu trực tiếp — `site.*`, `domain.*`, `cert.*` nếu cần mở rộng — nên coi bảng "Method namespaces"
của tài liệu là gợi ý, không phải nguồn cuối; đối chiếu bằng bindings đã vendor mới nhất hoặc gọi thử
lên daemon thật, đừng lặp lại việc phải xác nhận từng namespace một bằng tay.

Không có source nào "mới hơn" một cách đáng tin: `bindings/` là bản MixDB này chép lúc Pha 1, còn hai
file `.md` là snapshot lấy về hôm nay nhưng viết bởi người, sửa chậm hơn code — đúng cùng một bệnh
roadmap này vừa tự mắc ở header của nó. Kết luận rút ra không phải "tin cái nào" mà là: **đừng viết
code dựa một mình vào tài liệu — chạy `npm run bindings` lấy bản mới nhất trước khi bắt đầu Pha này,
và với bất cứ method nào bảng trên không khớp, hỏi `mixengined` thật** (`daemon.status` không liệt kê
method, nhưng một `POST /rpc` gọi thử với `method` sai trả về `error.data.code = "not_found"`
hay tương đương — cách rẻ nhất để biết một method có tồn tại không cần đọc mã nguồn Rust bên kia).
Xem **Quyết định D1**.

### Hai câu hỏi ban đầu đã được trả lời — và một bài học rút ra từ chính lỗi của spec này

Bản nháp đầu của spec này liệt kê ba câu hỏi mở. Hai câu đã được xác nhận:

- **PHP extension toggle có thật.** `runtime.list_extensions`/`runtime.set_extension` tồn tại;
  `ExtensionChoice { kind, version, name, enabled }` là tham số ghi, `ExtensionChange { extension,
  pool: PoolOutcome }` là câu trả lời — cả ba type **đã có sẵn trong `bindings/` đã vendor của chính
  repo này**, tự đọc lại xác nhận được, không phải tin theo lời kể. Lỗi ở bản nháp đầu: lọc danh sách
  file theo tên bắt đầu bằng `Extension` rồi kết luận cả cụm đó thuộc namespace `extension.*` (hệ
  blueprint/plugin Pha 4) — trong khi `ExtensionChoice`/`ExtensionChange` là một họ khác, thuộc
  `runtime.*`, chỉ trùng tiền tố tên. Bài học: **lọc theo tên file không thay được việc đọc doc comment
  của từng type** — bindings có gần 300 file, chỗ nhầm này có thể còn chỗ khác chưa lộ ra. Xem mục 2
  cho luồng UI, và **Rủi ro**.
- **`LogExcerpt` không liên quan tới trang Logs — đúng như nghi ngờ ban đầu.** Nó là field
  `daemon_log` bên trong `Manifest` (`daemon.bundle`, Pha 4), trả lời "trích đoạn `daemon.log` nào
  được đưa vào gói chẩn đoán" — không liên quan gì tới việc xem log service theo thời gian thực. Mục 4
  không dùng type này.

Câu thứ ba (`package.*` có vào Pha này không) được trả lời **có**, với yêu cầu cụ thể: luồng cài/gỡ
giống hệt Runtimes. Xem mục 2.

### Ba thứ Pha 1–2 để lại và Pha 3 dựng lên trên

- **Stream sự kiện đã mang `job_progress`/`job_finished` từ Pha 1**, chưa ai vẽ. `elevation.grant` và
  `daemon.doctor_repair` đã đẻ job từ Pha 1–2 nhưng luôn gọi kiểu chờ xong; Pha 3 là nơi đầu tiên một
  job thật sự chạy lâu (tải một bản PHP hay một package) và tiến độ phải lên màn hình khi nó đang chạy.
- **`ServiceSummary` (Dashboard, Pha 1) đã đủ để dẫn vào Services chi tiết** — `id`, `state`, `pid`,
  `port`, `depends_on`. Không cần gọi gì thêm để vẽ hàng có thể bấm vào; `service.limits`/
  `service.logs` chỉ gọi khi đã vào trang chi tiết.
- **`readSecrets`/`resolveKeyringRef` của module `db`** (Pha 0,
  [savedConnections.ts:47,80](../../apps/desktop/src/modules/db/savedConnections.ts)) đã biết đọc một mật khẩu
  từ namespace `mixengine` bằng đúng cặp `service`/`key` một `SecretAddress` mang. Mục 3 dùng lại
  nguyên hàm này, không viết đường đọc keyring thứ hai.

## 1. Projects

`project.list` → `ProjectList { projects: ProjectSummary[] }` — mỗi hàng: `name`, `root`,
`created_at`, `manifest?` (có `mixengine.toml` hay không — cái này quyết định pin trong file có hiệu
lực hay chưa), `keep_warm`.

- **Chi tiết.** `project.show` (`ProjectQuery { project: ProjectRef::Name }`) →
  `ProjectDetail { project, pins: ProjectPin[] }`. **`pins` là pin hiệu lực, không phải pin đã lưu** —
  mỗi `ProjectPin { kind, constraint, source: PinSource, resolved?, hint? }` nói pin này tới từ
  `mixengine.toml` hay từ row, và version nào nó thật sự trỏ tới trên máy này (`resolved`) — hoặc câu
  lệnh sửa (`hint`) khi không có bản nào khớp. Vẽ đúng bốn field này, không chỉ vẽ `constraint`: một
  panel chỉ hiện "PHP: 8.3" mà không nói pin đó có thật sự resolve được không là một panel nói dối
  đúng lúc người dùng cần nó nhất.
- **Tạo.** `project.create` (`ProjectCreate { root, name?, pins? }`) → `ProjectSummary`. `root` bắt
  buộc là thư mục **tuyệt đối và đã tồn tại** — dùng lại dialog chọn thư mục
  (`@tauri-apps/plugin-dialog`) `SiteForm.tsx` đã dùng cho doc root, không viết validate path tay.
  `name` bỏ trống thì daemon tự lấy từ `mixengine.toml` rồi từ tên thư mục — form không cần đoán
  trước, chỉ gửi những gì người dùng gõ. `pins` để trong một phần "Nâng cao" gấp lại mặc định — hầu
  hết project không cần pin lúc tạo, `mixengine.toml` hoặc mặc định của máy đã đủ.
- **Sửa.** Method thật là `project.update` — xác nhận qua `rpc.rs`, không phải `project.set_runtime`
  như `daemon-and-ipc.md` ghi (verb đó không tồn tại). Một method duy nhất sửa cả bốn thứ
  (`ProjectUpdate { project, name?, root?, pins?, keep_warm? }`), không phải bốn method riêng.
  **`pins` thay thế toàn bộ, không merge** — gửi lại mọi pin hiện có cộng thay đổi, `{}` xoá hết pin,
  field vắng mặt giữ nguyên. `root` chỉ dùng khi thư mục đã chuyển chỗ thật — không phải nơi sửa lại
  tên hiển thị.
- **Xoá.** `project.delete` (`ProjectQuery`) → `ProjectRemoval { removed, root_kept, manifest_kept? }`.
  Hộp thoại xác nhận nói rõ **thư mục và `mixengine.toml` được giữ nguyên, chỉ gỡ đăng ký** — cùng
  triết lý `ServiceRemoval`/`SiteRemoval` đã có trong bindings (xoá registration, không đụng dữ liệu
  trên đĩa). **Chưa biết** xoá một project mà Sites đang trỏ tới (`SiteOwner::Project`) có bị refuse
  không hay để lại site mồ côi — không có field nào trên `ProjectRemoval`/`ProjectSummary` nói trước
  điều này như `PackageSummary.services` làm cho package. Chỉ đo được với daemon thật, xem **Kiểm
  thử**.

Tích hợp với Sites (Pha 2, đã xong): `SiteForm.tsx` không đổi cách gọi (vẫn `api.projects()` →
`project.list`), chỉ đổi nội dung gợi ý khi rỗng — trỏ sang tab Projects thay vì bảo gõ lệnh CLI. Xem
[i18n/vi.ts:69](../../apps/desktop/src/modules/mixengine/i18n/vi.ts).

## 2. Runtimes & Packages

Một sidebar item, hai tab con — không phải hai mục sidebar riêng. Lý do ở **Quyết định D5**: cùng hình
dạng RPC, cùng hình dạng job, tách thành hai màn hình là vẽ hai lần một thứ giống hệt nhau.

### Ngôn ngữ (`runtime.*`)

`runtime.list_installed` (lọc được theo `kind` qua `RuntimeFilter`, bỏ trống thấy cả bốn) →
`RuntimeSummary { kind, version, channel, path, installed_at, bytes, default }`, một hàng một bản đã
cài, bản mặc định được đánh dấu. `runtime.list_available` → `RuntimeCatalogue { runtimes, stale }` —
**`stale: true` phải vẽ ra** (Quyết định D3): một danh sách từ cache không refresh được vẫn là danh
sách dùng được, nhưng im lặng về nó là nói dối đã hỏi được mạng.

- **Cài.** `runtime.install` nhận `RuntimeTarget { kind, version }`, trả `JobSummary`. Vẽ thanh tiến
  độ ngay hàng của bản đó trong bảng "có thể cài" — không phủ spinner lên cả màn hình, đúng luật T1.9
  Pha 1 đã đặt cho service. Tiến độ đến từ `job_progress` trên stream **đã mở sẵn** của Dashboard/tab —
  không mở stream thứ hai; xem mục 3.
- **Gỡ.** `runtime.uninstall` nhận `RuntimeUninstall { kind, version, force? }`. Refuse mặc định khi
  một project pin đúng bản này; message nêu project nào — **Projects (mục 1) là chỗ người dùng đọc
  tiếp để sửa pin đó**, không phải một câu lỗi cụt. `force: true` **chỉ vượt qua cái refusal đó** —
  không vượt qua một service đang chạy trên bản này, vì không service nào phụ thuộc trực tiếp một
  *runtime* (chỉ phụ thuộc lẫn nhau qua `depends_on`) nên không có xung đột thứ hai để `force` phải lo.
  UI hỏi xác nhận một lần trước khi gửi `force: true`.
- **Mặc định.** `runtime.set_default` nhận `RuntimeTarget`, trả `RuntimeSummary` — nút "Đặt mặc định"
  trên một hàng đã cài chưa phải mặc định.
- **PHP extension — có thật, xác nhận qua `bindings/` đã vendor của chính repo này** (xem Hiện trạng).
  `runtime.list_extensions` → `RuntimeExtension[] { name, linkage, enabled, source }` mỗi bản PHP.
  Toggle: `runtime.set_extension` nhận `ExtensionChoice { kind, version, name, enabled }`, trả
  `ExtensionChange { extension, pool: PoolOutcome }`. UI đọc `pool` sau mỗi lần bấm:
  - `"reloaded"` — im lặng, pool đã tự nhận cấu hình mới, không cần báo thêm.
  - `"restart_required"` — banner "Cần khởi động lại pool để có hiệu lực" kèm nút Restart, gọi
    `service.restart` (đã có từ Pha 1) cho đúng service của pool đó.
  - `"pool_not_running"` — câu "Đã lưu, sẽ áp dụng lần pool tiếp theo khởi động", không phải lỗi.
  `linkage: "static"` (một phần của binary, không tắt được) ẩn hẳn công tắc, không vẽ rồi disable nó —
  một control luôn từ chối là một control không nên có mặt.

### Dịch vụ (`package.*`)

`package.list` → `PackageList { packages: PackageSummary[] }`; mỗi `PackageSummary` mang
`services: ServiceId[]` — **service nào đang là instance của đúng version này**, hiện ngay trên hàng
để người dùng biết trước khi định gỡ. `package.list_available` (lọc theo `package` qua `PackageFilter`)
→ `PackageCatalogue { packages, stale }` — cùng component `stale`-badge với Ngôn ngữ ở trên (D3).

- **Cài.** `package.install` (`PackageTarget { package, version }`) → `JobSummary` — **dùng lại y hệt**
  component tiến độ job của Ngôn ngữ, chỉ khác namespace gọi. Đây chính là điều bạn yêu cầu: "package
  cần một luồng cài đặt y hệt Runtimes".
- **Gỡ.** `package.uninstall` (`PackageTarget`) → `PackageRemoval { removed }`. **Khác `runtime.uninstall`
  một điểm quan trọng: không có `force`.** `services` không rỗng thì refuse là chốt hẳn — không có
  cách vượt qua từ UI. Vẽ đúng danh sách service đang phụ thuộc, không vẽ nút "vẫn gỡ" vì không có gì
  để nút đó gọi. Xoá/chuyển service khỏi package trước là việc của `mix service delete`, ngoài Pha này
  (xem Phi mục tiêu).
- **Không có "đặt mặc định" cho package** — khái niệm đó chỉ có ở runtime (`RuntimeSummary.default`);
  một service instance chọn version của nó lúc `service.create`, không có "version mặc định của
  MariaDB" theo nghĩa toàn máy.

## 3. Job — hạ tầng đã có sẵn, tự đọc lại code trước khi định viết thêm

**Sửa lại so với bản nháp trước: `applyJob`/`JobRow` đã tồn tại trong `daemonState.ts`, và
`Dashboard.tsx` đã vẽ một danh sách job kèm `<progress>` từ Pha 1/2 — không phải "lần đầu module này
vẽ progress bar" như bản nháp trước viết. Đây là đúng loại lỗi Rủi ro của spec này đã tự cảnh báo: đọc
tên module rồi đoán nội dung, thay vì mở file ra đọc.** Việc của Pha 3 **không phải** viết một
`jobState.ts` mới — là dùng lại `applyJob(jobs: JobRow[], raw: string): JobRow[]` và
`interface JobRow { id, kind, percent, message }` đã có, đúng chỗ `daemonState.ts` đã có
`rowsFrom`/`applyEvent`/`needsResync` cho service.

- **Dashboard đã hiện job của Runtimes/Packages miễn phí, không cần sửa gì ở đó** — `applyJob` không
  lọc theo `kind`, một `runtime.install` hay `package.install` tự động lên danh sách job chung của
  Dashboard ngay khi nó chạy, y hệt cách `elevation.grant`/`daemon.doctor_repair` đã lên đó từ trước.
- **Màn hình Runtimes & Packages cần vẽ tiến độ riêng, ngay trên hàng của phiên bản đang cài** — đây
  mới là phần thật sự mới. Màn hình tự mở `api.watch()` của chính nó (mỗi màn hình tự quản lý
  watch/unwatch, đúng pattern `Dashboard`/`Sites` đã theo), áp `applyJob` lên một `JobRow[]` cục bộ, và
  giữ thêm một map cục bộ `installingJob: Record<VersionKey, number>` — gán ngay khi
  `runtime.install`/`package.install` trả về `JobSummary.id`, dùng để tra `jobs.find(j => j.id ===
  installingJob[key])` cho đúng hàng. `JobRow` không mang theo version/kind của package — đây là lý do
  cần map cục bộ, không phải thứ `daemonState.ts` phải biết.
- **Không cần `job.list` hay `job.status` để vẽ một job vừa tự mình tạo ra** — id đã có ngay trong câu
  trả lời của `runtime.install`/`package.install`, và mọi bước tiếp theo tới qua stream đang mở sẵn.
- **Cần `job.status` (hoặc `job.list` lọc `state: running`) đúng một lần: lúc mở màn hình.** Một job
  đang chạy từ trước khi màn hình này mở (cài một bản PHP từ CLI, rồi mở MixDB) không có `job_progress`
  nào cho UI thấy nó bắt đầu — nhưng vì `JobRow` không mang version, một job "mồ côi" kiểu này không
  có hàng nào để gắn vào; cách xử lý thực tế là bảng "có thể cài" tự đọc `installed`/`stale` lại khi
  focus quay lại tab, không cố gắn job cũ vào một hàng.
- `job.cancel` — không dùng ở Pha này. Không method nào của Pha 3 sinh ra một job nên huỷ nửa chừng là
  an toàn (một bản tải dở không để lại service nào đang chạy dở); để lại cho Blueprints (Pha 4), nơi
  `RunScaffold` thật sự có thể cần huỷ.

## 4. Services chi tiết

Một trang mở từ một hàng `ServiceSummary` ở Dashboard — không phải sidebar riêng, `servicesDetail`
trong `Sidebar.tsx` là chỗ chứa, nhưng vào bằng cách bấm một service cụ thể chứ không phải một danh
sách riêng (danh sách đã có ở Dashboard từ Pha 1, lặp lại nó là hai bảng phải đồng bộ).

### Giới hạn (T3.3 — chắc chắn buildable, cả hai nguồn tài liệu khớp nhau)

`service.limits` (đọc) và `service.set_limits` (ghi, **toàn bộ `ResourceLimits`, không phải patch** —
`ServiceLimitsSet`'s doc nói rõ: gửi lại cả ba field kể cả cái không đổi, hoặc mất field kia) đều trả
`ServiceLimitsReport { service, limits, support, watchdog }`.

- `support: LimitSupport` nói **CPU và memory có thể khác nhau trên cùng máy** (`Enforcement` đóng 4
  biến thể: `hard`, `unsupported`, `unavailable { why }`, `advisory { why }`) — vẽ hai control độc
  lập, không một cặp khoá chung. `hard` vẽ một wall (chạm trần là bị giết); `advisory` vẽ một vạch
  cảnh báo, **không được vẽ như một bảo đảm** — đúng câu luật roadmap đã chốt.
- `watchdog?: MemoryWatchdog` — `null` gộp hai trường hợp khác nhau (máy tự ép được, hoặc service
  không khai `memory_mb`) làm một câu trả lời: không có gì đang canh. Không suy ra cái nào từ `support`
  ở phía client; nếu cần phân biệt hai lý do, đó là việc mới không phải Pha này.
- `priority: Priority` (`"normal" | "background"`) — một switch, `support.priority` nói có tác dụng gì
  không trên máy này.

### Idle (một field, ba trạng thái)

`service.set_idle` nhận `ServiceIdleSet { service, minutes? }`. **Không phải hai state (bật/tắt) mà ba**:
`null` (theo recipe), `0` (tắt hẳn bất kể recipe), `n` (n phút). Một dropdown ba lựa chọn + ô số khi
chọn "n phút", không một checkbox.

### Database — T3.4 và T3.5

- **Tạo.** `database.create` (`DatabaseCreate { service, database, user? }`) → `DatabaseAccount
  { service, database, user, secret: SecretAddress, made: Provisioned }`. `made` nói cái nào **đã có
  sẵn** và cái nào **mới tạo** — vẽ hai câu khác nhau ("database đã có từ trước" vs "vừa tạo"), không
  gộp thành một "thành công". Không bao giờ nhận mật khẩu về; chỉ địa chỉ keyring.
- **Client có mở được không.** `database.client` (đọc, không khởi động gì) →
  `DatabaseClientReport { service, protocol?, secret?, client }`. Ba trạng thái của `client`
  (`installed`/`not_installed`/`no_client`) và `protocol: null` (service không client nào mở, ví dụ
  memcached) đều là **trạng thái phải vẽ**, không phải lỗi — đúng luật roadmap đã ghi hai lần.
- **"Open" — không đi qua `database.open`.** Đây là điểm khác với luồng OS-handoff Pha 0.
  `database.open` khởi động một **process ngoài** với `DesktopClient` tìm được — và với service kiểu
  `mysql`/`postgres`, `DesktopClient` đó chính là MixDB (`extension: "desktop-app"`, `name: "MixDB"`,
  đã đăng ký từ Pha 0). Gọi `database.open` từ trong MixDB nghĩa là MixDB tự bảo daemon **mở một tiến
  trình MixDB khác** — vòng ra ngoài rồi vòng lại, đúng thứ roadmap T3.5 nói "không nên đi qua OS".

  **Sửa lại so với bản nháp trước: cơ chế mở tab đã có sẵn, không cần API mới — bản nháp trước đọc
  nhầm `shell/launch.rs`/`launch.ts` là "chỉ dành cho OS-handoff", trong khi đọc lại code thì
  `crate::launch::request` là một hàm Rust bình thường, gọi được từ bất kỳ command nào, không chỉ từ
  chỗ nhận URL `mixdb://`.** Pha 0 đã dùng đúng nó cho việc này: `handoff::accept()`
  ([handoff.rs:189-206](../../apps/desktop/src-tauri/src/modules/db/handoff.rs)) dựng một `Handoff`, gọi
  `HandoffState::keep()` lấy một id, rồi gọi thẳng `crate::launch::request(app, TabRequest { module_id:
  "db", state: json!({"handoffId": id}) })` — không có gì trong hàm đó nhắc tới nguồn gốc URL. Pha 3
  chỉ cần lặp lại đúng ba bước đó **từ phía trong**, không qua URL:

  1. Gọi `database.client` (`DatabaseClientReport { protocol, secret, client }`).
  2. Nếu có `secret: SecretAddress`, gọi thẳng `crate::secrets::secrets_resolve_mixengine(secret.key)`
     — hàm Rust `pub async`, gọi được trực tiếp, không cần round-trip qua frontend rồi quay lại.
  3. Dựng một `Handoff { config: ConnectionConfig { kind: <map từ DatabaseProtocol>, host:
     "127.0.0.1", port: <ServiceSummary.port>, username, password, database: None, .. }, label:
     <ServiceId>, keyring_ref: Some(secret.key) }`, `HandoffState::keep()`, rồi
     `crate::launch::request(..., TabRequest { module_id: "db", state: json!({"handoffId": id}) })`.

  `DbTab.tsx` đã biết đọc `{"handoffId": ...}` từ Pha 0 ([DbTab.tsx:506-525](../../apps/desktop/src/modules/db/DbTab.tsx))
  — không sửa gì bên `db`. Mật khẩu không đi qua frontend một lần nào, kể cả tạm thời: nó chỉ sống
  trong tiến trình Rust từ lúc đọc keyring tới lúc nằm trong `Handoff` đang chờ `db` lấy. Đây là quyết
  định của Pha này — xem **Quyết định D2** (thay cho hai hướng spec bản trước còn để ngỏ).

## 5. Logs

`GET /logs/{service_id}?tail=N&follow=1` theo `daemon-and-ipc.md` đọc hôm nay; `LogSubject` đã vendor
lại nói route là `GET /logs/service/{id}` (hai đoạn, tách khỏi `GET /logs/job/{id}`) — một khác biệt
nữa giữa hai nguồn, xem **Quyết định D1**. `LogFrame` framed SSE giống `/events`:

- `{"type":"line", stream, at, text}` — một dòng thật, `stream: "stdout" | "stderr"`.
- `{"type":"historic", text}` — dòng từ trước khi kết nối, không có timestamp/stream (đã có trong text
  nếu service tự ghi).
- `{"type":"gap", missed}` — client chậm hơn service, số dòng bị bỏ. Vẽ một dòng phân cách kiểu "— bỏ
  qua N dòng —", không im lặng nuốt.

`tail=N` một mình là ảnh chụp rồi đóng kết nối; `follow=1` giữ sống. UI: mở trang Logs của một service
là `tail=200&follow=1` một lần; nút "Xem thêm phía trên" gọi lại với `tail` lớn hơn — **không phải
phân trang qua daemon**, đây vẫn là log ring trong bộ nhớ của daemon (`LogPolicy.ring_lines`), không
phải file.

**Log không bao giờ trộn vào `/events`** (ADR 0009, nhắc lại từ roadmap) — hai kết nối SSE độc lập
mở song song khi trang Logs đang mở, đóng khi rời trang.

**Không có nút "Mở thư mục chứa log" — xác nhận, không phải thiếu sót.** Roadmap T3.6 viết "lộ luôn
đường file trên đĩa", nhưng không field nào trên `LogLine`/`LogFrame` mang một đường dẫn, và đây là
chủ đích của ADR 0009: daemon không bao giờ trả layout lưu trữ của nó cho client — một phần vì một
client không cùng máy (thiết kế tương lai) không có gì để mở đường dẫn đó. `LogExcerpt` cũng không
phải type của trang này (xác nhận ở Hiện trạng) — nó thuộc `daemon.bundle`, Pha 4. MixDB tiêu thụ log
**chỉ qua stream** (`tail`/`follow`), không có đường vòng qua file — kể cả khi daemon chạy cùng máy.
Roadmap nên bỏ câu "lộ luôn đường file trên đĩa" khỏi T3.6.

## Kiểm thử

Phần thuần, không cần daemon nào:

| Test | Nội dung |
| --- | --- |
| `projectPins` | vẽ đúng `source` (file/row) và `resolved`/`hint` của mỗi `ProjectPin`; không suy ra `resolved` từ `constraint` phía client |
| `installingJob` map (trong `runtimeState.ts` hay tương đương, không phải `applyJob` — cái đó đã có test riêng) | gán đúng `job.id` vào đúng `VersionKey` sau khi `install` trả lời; tra đúng hàng từ `JobRow[]` đã áp `applyJob` |
| `poolOutcome` | ba giá trị `PoolOutcome` map đúng ba cách vẽ; `"pool_not_running"` không bị vẽ như lỗi |
| `logState`/parser SSE `/logs` | `line`/`historic`/`gap` phân biệt đúng; `gap` không làm mất các dòng trước nó |
| `limitsForm` | `support.cpu = "unsupported"` ẩn control CPU; `advisory` vẽ khác `hard`; gửi lại luôn cả ba field của `ResourceLimits` |
| `idleSelect` | ba trạng thái map đúng `null`/`0`/`n` hai chiều |
| Đường "Open in MixDB" | dựng đúng `ConnectionConfig` từ `DatabaseClientReport` + mật khẩu resolve được; `protocol: null` hoặc `client: "no_client"` không hiện nút |

**Không test được bằng vitest**, cần MixEngine thật: `package.*` có đúng như bindings suy ra không —
đây là namespace duy nhất trong bảng ở Hiện trạng còn chưa đối chiếu trực tiếp với `rpc.rs`;
`runtime.install`/`package.install` một bản thật có tiến độ hợp lý không hay nhảy thẳng 0→100; xoá một
project mà site đang trỏ tới có bị refuse hay để lại site mồ côi.

## Rủi ro

- **Method không tồn tại như tài liệu nói.** Bảng ở Hiện trạng liệt kê năm chỗ lệch; viết `commands.rs`
  cho một method đoán sai tên là một buổi tối debug một `not_found` không rõ do sai tên hay do daemon
  cũ. Giảm bằng D1.
- **Lọc `bindings/` theo tiền tố tên rồi kết luận vội — đã xảy ra thật một lần ở đây.** Bản nháp đầu
  loại bỏ toàn bộ cụm `Extension*` vì đoán chúng thuộc `extension.*` (Pha 4), trong khi
  `ExtensionChoice`/`ExtensionChange` thuộc `runtime.*`. Gần 300 file trong `api/types/`, việc này có
  thể còn lặp lại ở một góc khác của Pha 3 hay Pha 4 — trước khi kết luận một khả năng "không có
  method nào hỗ trợ", đọc doc comment của từng type nghi ngờ, không chỉ đọc tên file.
- **`force` của `runtime.uninstall` bị hiểu nhầm như `force` của `service.delete`** (Pha 2 chưa dùng,
  nhưng `ServiceDelete.force` cùng tên field, nghĩa khác — vượt qua site declare, không vượt qua
  project pin). `package.uninstall` **không có** `force` gì cả — ba khái niệm trông giống nhau, ba
  nghĩa khác nhau. Một hằng số hay một hàm helper dùng chung tên `force` cho cả hai chỗ có nó là điểm
  dễ lẫn nhất file này để lại cho code review.
- **`HandoffState` là app state dùng chung, không phải của riêng module `db`.** Command mới của
  `database.client`/"Open" (mục 4) đọc/ghi state đó qua `crate::modules::db::handoff::HandoffState` từ
  bên ngoài `modules/db/` — hợp lệ (đã `pub`, đã `.manage()` một lần ở `modules/db/mod.rs`), nhưng là
  chỗ duy nhất module `mixengine` chạm vào state của module khác. Đáng một dòng comment tại chỗ gọi
  giải thích tại sao, để không ai tưởng nhầm là vi phạm luật "không module nào biết khái niệm của
  module khác" ([`eslint.config.js`](../../apps/desktop/eslint.config.js) chỉ canh phía TypeScript, không canh
  phía Rust).

## Quyết định

**D1 — Ba việc cụ thể đã xác nhận bằng `rpc.rs` thật, không cần hỏi thêm; vẫn vendor lại `bindings/`
trước khi viết `commands.rs` cho phần còn lại.** Đã chốt: method sửa project là `project.update`, không
phải `set_runtime`; `service.config_get`/`config_set` **không tồn tại**, và đây là chủ đích kiến trúc
("config sinh ra dùng một lần, không phải blob đọc-ghi chung") chứ không phải chỗ chờ code — sửa
port/bind/data_dir/autostart của một service đã tồn tại ở ngoài phạm vi vĩnh viễn, không phải "chưa
tới lượt"; `GET /logs/...` không mang path nào, theo đúng ADR 0009, nút "Mở thư mục" không viết được.
`npm run bindings` (script có sẵn từ D2 của Transport spec) vẫn nên chạy trước khi code, cho phần
Pha này chưa đối chiếu trực tiếp với `rpc.rs` — cụ thể là `package.*` (bảng ở Hiện trạng còn ghi "chưa
tra") và mọi type mới `runtime.list_extensions`/`set_extension` có thể kéo theo mà bindings hiện tại
chưa vendor đủ.

**D2 — "Open in MixDB" dùng lại nguyên đường Pha 0 đã đi: `Handoff` + `HandoffState` +
`crate::launch::request`, gọi thẳng từ một Tauri command mới, không qua URL.** Không phải hai hướng
spec bản trước cân nhắc ("API event bus mới" so với "mượn hàng đợi OS-handoff") — cả hai giả định sai
rằng `launch::request` gắn với nguồn gốc OS. Đọc lại `launch.rs`: đó là một hàm nhận `TabRequest {
module_id, state }` rồi đẩy vào một hàng đợi và bắn một sự kiện — không tham số nào nói tới URL hay
tiến trình khác. `handoff::accept()` (Pha 0) đã gọi đúng nó theo cách này rồi; Database (Pha 3) gọi lại
y hệt, chỉ khác nguồn tạo `Handoff` là `database.client` + `secrets_resolve_mixengine` gọi thẳng trong
Rust thay vì đọc một URL `mixdb://`. Không thêm API mới ở `shell/`, không sửa `DbTab.tsx`. Xem mục 4
cho các bước cụ thể.

**D3 — `RuntimeCatalogue.stale` và `PackageCatalogue.stale` vẽ cùng một component.** Cùng hình dạng,
cùng lý do tồn tại (cache không refresh được vẫn dùng được, im lặng về nó là nói dối), nên một badge
"Danh sách có thể cũ" nhận `stale: boolean` chung cho cả hai tab thay vì viết hai lần.

**D4 — Projects là mục sidebar thứ hai, ngay sau Dashboard, trước Sites.** Site cần chọn Project lúc
tạo, nên thứ tự sidebar nên đi trước thứ nó phục vụ. Đây là mục **ngoài** 9 màn hình `client-surface.md`
liệt kê — comment ở [Sidebar.tsx:6](../../apps/desktop/src/modules/mixengine/components/Sidebar/Sidebar.tsx)
("Chín mục cố định của `client-surface.md`") phải sửa lại, ghi rõ Projects là một mục MixDB tự thêm và
vì sao (Sites không dùng được nếu không có project nào, và `project.*` đã đủ method cho một màn hình
đầy đủ chứ không phải nửa vời).

**D5 — Runtimes và Packages là một sidebar item, hai tab con — không phải hai mục sidebar.** Cùng hình
dạng RPC (`RuntimeSummary`~`PackageSummary`, `RuntimeCatalogue`~`PackageCatalogue`, `RuntimeTarget`~
`PackageTarget`), cùng hình dạng job, khác đúng namespace gọi và đúng một khả năng (`force` chỉ
`runtime.uninstall` có). Tách hai mục sidebar là vẽ hai lần một thứ giống hệt nhau và làm sidebar phình
thêm một mục nữa ngoài 9 mục gốc — Projects (D4) đã là một ngoại lệ, không nên thành tiền lệ cho mỗi
namespace mới.

Nhãn của mục đó là **Packages**, không phải *Runtimes*: màn này chứa cả web server, cơ sở dữ liệu
và cache, và một nhãn chỉ nói "runtime" thì sai với nửa nội dung. Khoá màn trong code cũng là
`packages`, còn thư mục `screens/Runtimes/` giữ tên vì nó đã chứa `Packages.tsx`.

**D6 — `package.uninstall` bị refuse thì dừng ở đó, không có bước hai.** Không giống
`runtime.uninstall` có `force` để vượt qua refusal-vì-pin, `package.uninstall` không có tham số nào
tương tự — refuse vì `services` không rỗng là chốt. UI vẽ danh sách service đang phụ thuộc và một câu
giải thích, không vẽ một nút "vẫn gỡ" gọi vào chỗ không có gì nhận nó.
