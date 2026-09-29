---
status: implemented
date: 2026-09-06
---

# Blueprints và Extensions: hai màn hình đầu của Pha 4, module `mixengine`

Ngày 2026-09-06. Một phần của Pha 4 trong [roadmap/mixengine-module.md](../roadmap/desktop/mixengine-module.md)
(T4.3, T4.4) — **không phải cả Pha 4**. Metrics và Settings bị giữ lại, xem "Nợ" ở cuối: ba câu hỏi
gửi MixEngine chưa có trả lời, và cả hai màn hình đó phụ thuộc câu trả lời theo cách Blueprints/
Extensions không phụ thuộc. Khi cả bốn màn hình xong, header của roadmap nên sửa thành "Pha 4 đã
xong" theo đúng bài học Pha 0 để lại — *"Code là câu trả lời, note không phải"* — không phải việc
của spec này.

## Mục tiêu

- **Blueprints**: capture một project đang có, liệt kê blueprint đã lưu (kèm huy hiệu tin cậy), nhập
  một blueprint người khác viết, và apply một blueprint lên một project mới — dry-run trước, rồi thật.
- **Extensions**: duyệt registry, cài (từ registry hoặc từ một thư mục cục bộ), gỡ, start/stop một
  extension kiểu `service`.

## Phi mục tiêu

- Metrics, Settings — xem "Nợ".
- Đổi cấu hình một extension đã cài. **Xác nhận không tồn tại**, không phải chưa tới lượt: không có
  `extension.configure` nào trong `rpc.rs` thật, dù roadmap T4.4 ("install/uninstall/configure") và
  `client-surface.md` mục 8 ("per-extension settings") đều ngụ ý có. Xem Quyết định D1.
- `extension.inspect` — đọc một manifest thô, không cần máy này trả lời được gì. Màn hình Install
  không cần bước này: `extension.plan` đã trả về mọi thứ `inspect` có, cộng thêm phần máy này mới
  biết (`client`, `signed`, đường cài đặt thật) — gọi cả hai là đọc file hai lần. Xem Quyết định D2.
- `blueprint.export`, `blueprint.delete`. **Xác nhận không tồn tại** — xem Hiện trạng.
- Sửa `runtime.set_extension`/`ExtensionsPanel.tsx` (PHP extension theo từng bản, Pha 3) — tên trùng
  `extension.*` thuần trùng hợp, xem Rủi ro.

## Hiện trạng

### Đối chiếu ba nguồn — bindings đã vendor, `client-surface.md`/`daemon-and-ipc.md` tải lại hôm nay,
### và `crates/mixengine-daemon/src/api/rpc.rs` + `crates/mixengine-proto/src/rpc.rs` (đọc trực tiếp
### từ `mixnz/mixengine@master`, không tin lời kể của hai file `.md`)

Bài học pha trước ("đừng viết code dựa một mình vào tài liệu") đẩy thêm một bước: `rpc.rs` phía
*client* trong chính repo này ([src-tauri/.../rpc.rs](../../apps/desktop/src-tauri/src/modules/mixengine/rpc.rs))
chỉ là một hàm gọi JSON-RPC chung, không biết method nào tồn tại — spec pha trước ghi "xác nhận qua
`rpc.rs`" nhưng không nói rõ *file nào*. Spec này tải thẳng `crates/mixengine-proto/src/rpc.rs` (bảng
hằng số method) và `crates/mixengine-daemon/src/api/rpc.rs` (nơi chúng được đăng ký) từ
`mixnz/mixengine@master` — đây mới là nguồn không thể lệch, vì chính code này quyết định daemon nhận
method nào.

**Ba chỗ `daemon-and-ipc.md` (bản tải hôm nay) sai, cả ba đều đã ảnh hưởng roadmap:**

| Namespace | `daemon-and-ipc.md` viết | `mixengine-proto::rpc` thật |
| --- | --- | --- |
| `blueprint.*` | `list, capture, apply, export, import, delete` | **Chỉ bốn**: `capture, import, list, apply` — không `export`, không `delete` |
| `extension.*` | `registry_list, install, uninstall, start, stop, configure` | **Tám, tên khác hẳn**: `inspect, list, available, plan, install, uninstall, start, stop` — không `registry_list` (tên thật `available`), không `configure` |
| — | — | `extension.plan` không nằm trong danh sách namespace của `daemon-and-ipc.md` dù có mô tả riêng ngay dưới bảng — dễ đọc lướt mà bỏ sót |

`BlueprintCapture`'s doc comment tự xác nhận vế đầu ("There is no `blueprint.delete` in this
build"), nên đây không phải suy luận từ một danh sách hằng số thiếu sót — là kiến trúc cố ý. Không có
bằng chứng tương đương cho `extension.configure`, nhưng sự vắng mặt của nó trong cả bảng hằng số lẫn
bindings đã vendor (không type `ExtensionConfigure` nào) là hai nguồn độc lập cùng nói không.

**Toàn bộ type cần cho hai màn hình này đã vendor sẵn** trong
[api/types/](../../bindings/) — không method nào ở đây cần
`npm run bindings` trước khi viết `commands.rs`, khác Pha 3 (nơi `package.*` còn phải đối chiếu).

## 1. Blueprints

### Capture

`blueprint.capture` (`BlueprintCapture { project: ProjectRef, name, description?, overwrite }`) →
`BlueprintSummary`. `project` tái dùng đúng dropdown `project.list` màn hình Sites (Pha 2)/Projects
(Pha 3) đã dùng — `ProjectRef::Name` gửi tên đã chọn, không phải `ProjectRef::Path` (đường đó dành
cho CLI đứng trong thư mục project, MixDB luôn biết tên qua danh sách). `overwrite` mặc định `false`
— **không có `blueprint.delete`, nên một slug gõ nhầm và không tick overwrite là vĩnh viễn kẹt ở tên
đó**; dialog capture nên cảnh báo rõ hơn form Save thông thường, không chỉ một checkbox im lặng.

### List

`blueprint.list` → `BlueprintList { blueprints: BlueprintSummary[] }`. Ba cột ngoài tên/mô tả:

- **`trusted`** — huy hiệu, không phải icon khoá chung chung: `builtin`/`captured` luôn `true` (máy
  này tự viết ra hoặc đi kèm bản build), `imported` là nơi duy nhất ra `false`.
- **`signature`** (`SignatureCheck?`) — `"verified"`/`"missing"`/`"rejected"`, chỉ có ý nghĩa cùng
  `source: "imported"`. Ẩn cột này (không vẽ trống) cho `builtin`/`captured`, vì `None` ở đó nghĩa là
  "chưa từng kiểm", không phải "kiểm rồi và sạch" — vẽ một dấu tích sẽ nói ngược sự thật.
- **`source`** — `builtin`/`captured`/`imported`, ba chữ hiển thị thẳng, không cần bản dịch phức tạp.

### Import

`blueprint.import` (`BlueprintImport { path, signature?, name?, overwrite }`). `path` qua dialog chọn
file `.toml` (tái dùng `@tauri-apps/plugin-dialog`, đúng cách `ProjectForm.tsx` chọn thư mục —
[ProjectForm.tsx:2](../../apps/desktop/src/modules/mixengine/screens/Projects/ProjectForm.tsx)). `signature`
để trống — daemon tự tìm `<path>.minisig` cạnh file. **Method này không bao giờ trả lỗi vì chữ ký
sai**: một file không ký hoặc ký sai vẫn nhập được, chỉ đổi `BlueprintSummary.signature` thành
`"missing"`/`"rejected"` và `trusted: false`. UI không hiện toast lỗi cho trường hợp này — hiện đúng
`BlueprintSummary` trả về, để bảng List tự nói phần "không ai bảo chứng".

### Apply — một method, hai lượt gọi

`blueprint.apply` nhận `BlueprintApply { blueprint, project, root, dry_run, answers?, scaffold? }`.

**Lượt 1 — `dry_run: true`.** `root` qua dialog chọn thư mục trống (tái dùng pattern `ProjectForm`/
`SiteForm` đã theo). Trả `BlueprintApplyResponse::Planned { plan: BlueprintPlan }`. Vẽ từng
`PlanStep { action: PlanAction, disposition: Disposition, elevates }` theo đúng thứ tự mảng —
`elevates: true` trên một dòng là dấu hiệu "lượt apply thật sẽ kèm đúng một prompt quyền", không phải
việc UI tự bật gì ở bước này. Sáu nhãn `PlanAction` cần một câu người đọc được mỗi loại
(`register_project`, `install_runtime`, `install_package`, `ensure_service`, `create_database`,
`create_site`, `add_domain`, `issue_certificate`, `set_php_extension`, `run_scaffold`) — bảng ánh xạ
action → câu là việc của `commands.rs`/i18n, không liệt kê hết ở đây.

`Disposition` quyết định UI cần hỏi gì thêm trước khi gửi lượt 2, **và chỉ hai trong sáu biến thể cần
một câu trả lời**:

- `satisfied`/`create` — không cần gì, bước sẽ tự chạy.
- **`choice { installed, wanted }`** — bản đã cài không khớp constraint blueprint xin. Hỏi "dùng bản
  đã cài hay cài đúng bản kia", gom thành `VersionAnswer { subject: AnswerSubject, answer:
  MismatchAnswer }` (`"install" | "use_installed"`, không có "huỷ" — không gửi gì là huỷ). `subject`
  suy từ chính step đó (`install_runtime`→`AnswerSubject::Runtime`, `ensure_service`→
  `AnswerSubject::Service` với `id` là `<package>@<instance>`).
- **`confirm { what }`** — luôn đi kèm `action: run_scaffold` (đây là *cách* T78a's "hỏi trước khi
  chạy lệnh của người lạ" xuất hiện trên plan, không phải một cơ chế đồng ý chung chung). Hiện đúng
  câu lệnh (`PlanAction::RunScaffold.command`) kèm trạng thái tin cậy của **plan**, không phải của
  blueprint nói chung — `BlueprintPlan.trusted`/`signature`, hai field lặp lại đúng lúc này vì đây là
  chỗ duy nhất chúng đổi thành hành động. Gửi `ScaffoldConsent { command, untrusted: !trusted }` —
  named đúng cái đã hiện, không phải một bit "đồng ý" rời khỏi ngữ cảnh.
- `blocked { reason }`/`unsupported { reason }` — hiện lý do, khoá nút Apply cho tới khi disposition
  đổi (đổi máy, cài thủ công bên ngoài, v.v. — không phải việc UI sửa được).

**Lượt 2 — `dry_run: false`, `answers`/`scaffold` đã gom.** Trả `BlueprintApplyResponse::Started {
job: JobSummary }`. **Không viết hạ tầng job mới** — tái dùng đúng `applyJob`/`JobRow` từ
[daemonState.ts:90](../../apps/desktop/src/modules/mixengine/daemonState.ts), cùng cách
[Languages.tsx](../../apps/desktop/src/modules/mixengine/screens/Packages/Languages.tsx)/
[PackageList.tsx](../../apps/desktop/src/modules/mixengine/screens/Packages/PackageList.tsx) đã vẽ tiến độ
`runtime.install`/`package.install` ở Pha 3: màn hình Apply tự mở `api.watch()`, áp `applyJob` lên
`JobRow[]` cục bộ, giữ `job.id` từ response để tra đúng hàng bằng `jobFor` (đã có ở
[runtimeState.ts:27](../../apps/desktop/src/modules/mixengine/runtimeState.ts)).

Khi job xong, `JobFinish` mang `BlueprintApplied { blueprint, project, root, steps: StepOutcome[] }`
— vẽ **mọi** step kể cả `already_true` (đúng nguyên tắc T78 để lại: một apply chạy lại toàn dòng
"đã sẵn từ trước" là bằng chứng lần đầu đã xong, không phải noise cần lọc). Output thật của lệnh
scaffold (nó in ra gì) không nằm trong `BlueprintApplied` — đọc qua `GET /logs/job/{id}`
(`LogSubject::Job`), **cùng cơ chế SSE với Logs màn hình Pha 3** (`LogFrame` với `line`/`historic`/
`gap`), không phải luồng mới: mở khi job đang `run_scaffold`/đã xong và người dùng muốn xem output,
đóng khi rời màn hình Apply.

## 2. Extensions

### Duyệt và đã cài

`extension.available` → `ExtensionCatalogue { extensions: ExtensionOffer[], unreadable, stale }`.
`stale` dùng lại đúng component badge Runtimes/Packages đã có (Quyết định D3,
[runtimes-services-logs-design.md](2026-09-06-mixengine-runtimes-services-logs-design.md)) — cùng lý
do tồn tại, không viết lần hai. `unreadable` (số entry build này không đọc được) hiện thành một dòng
phụ dưới bảng, không phải lỗi — đúng luật "mọi lý do bị bỏ sót phải nói ra" T4.8 áp cho `daemon.bundle`
cũng áp được ở đây.

`extension.list` → `InstalledExtensions { extensions: ExtensionSummary[] }`. Với extension
`kind: "service"`, trạng thái chạy (`running`/`stopped`/…) đọc từ `service.list` **đã có sẵn từ Pha 1**
qua `ExtensionSummary.service: ServiceId | null` — không gọi gì thêm, một extension `service` chỉ là
một hàng `service.list` đã biết, nhìn qua id khác. Với `web-app`, `ExtensionSummary.site` trỏ sang
Sites (Pha 2) để xem trạng thái domain/HTTPS — không lặp lại bảng đó ở đây.

### Cài

Một luồng cho cả hai nguồn: chọn một hàng từ bảng registry (`ExtensionOrigin::Registry { id }`) hoặc
nút "Cài từ thư mục…" mở dialog chọn thư mục có `extension.toml` (`ExtensionOrigin::Path { path }`,
dialog giống Import của Blueprints). Cả hai gọi `extension.plan(ExtensionPlanRequest { source })` →
`ExtensionPlan`. **Đây là bước duy nhất, không gọi `extension.inspect` trước** — xem Quyết định D2.

Vẽ plan trước khi bật nút "Cài":

- `permissions.services: ApiAccess[]` — **một disclosure, không phải quyền chặn được** (ADR 0014 phía
  MixEngine): liệt kê "sẽ gọi các API: đọc/ghi", không có nút "từ chối một quyền" nào có tác dụng.
  Đừng vẽ checkbox trông như thể tắt được.
  `permissions.network: NetworkReach` (`loopback`/`lan`), `permissions.filesystem:
  FilesystemReach[]` (`own-data`/`project-roots:read`) — ba dòng liệt kê, không phải bảng.
- `ports: PortWish[]` — "sẽ xin cổng N", **chưa giữ gì**, không vẽ như một xung đột cổng đã xảy ra.
- `install_dir`/`data_dir` — hai đường dẫn, đọc để hiển thị, không phải để sửa.
- `signed: boolean` — `false` cho mọi install `--path` (tức mọi `ExtensionOrigin::Path`); banner cảnh
  báo rõ, không phải một icon nhỏ cạnh tên.
- `homepage?` — một dòng, chỉ khi manifest có.
- **`site?: PlannedSite`** (kind `web-app`) — `domain`, `pool` (chính là pool riêng của extension đó,
  không phải pool project đang chạy — roadmap T82a: một web-app luôn có pool của chính nó), và
  **`signs_in?`**: nếu có, đây là field quan trọng nhất của cả plan — vẽ **giữa** danh sách quyền
  (`permissions`) chứ không cạnh `domain`, đúng luật T4.4 roadmap đã ghi, kèm đúng ba câu:
  tài khoản nào, mật khẩu đọc từ keyring lúc pool khởi động, không gì ghi xuống đĩa. `database?`
  (service quản lý) hiện ngay dưới, cùng khối.
- **`client?: DesktopPresence`** (kind `desktop-app` — `installed { program }` / `not_installed {
  searched }`). Xem cảnh báo tự-tham-chiếu ngay dưới trước khi vẽ nút Cài cho trường hợp này.

Đồng ý → `extension.install(ExtensionInstall { source, consent: ExtensionConsent { id, version,
signed, network } })` — bốn field của `consent` **phải đúng những gì plan vừa hiện**, không phải
điền lại từ input người dùng: registry có thể đổi giữa lúc plan và lúc install, và daemon từ chối một
consent không khớp registry hiện tại. UI giữ nguyên object `ExtensionPlan` vừa nhận, trích bốn field
đó ra khi gửi, không hỏi lại người dùng lần hai.

**Cảnh báo tự-tham-chiếu (`kind: "desktop-app"`, và entry đó là chính MixDB).** Roadmap T4.5 đã nêu
đúng vấn đề: MixEngine tìm ứng dụng desktop chứ không cài nó, và nếu registry có một entry đại diện
chính MixDB (cùng cơ chế Pha 0 dùng để MixEngine tự nhận diện MixDB qua `database.open`/
`DesktopClient`), màn hình Extensions của MixDB không được vẽ một nút "Cài đặt" cho chính ứng dụng
đang chạy nó. **Chưa xác nhận được cách nhận ra trường hợp này** — không có field nào trên
`ExtensionOffer`/`ExtensionPlan` tự nói "đây là bạn"; `ExtensionId` là kiểu duy nhất có thể so khớp
được, nhưng giá trị hằng số MixEngine dùng cho entry đó (nếu có thật) không nằm trong bất cứ tài liệu
nào đã đọc. Xem Rủi ro — cần đối chiếu registry thật trước khi khoá phần này.

### Gỡ, Start, Stop

`extension.uninstall(ExtensionUninstall { id, delete_data })` → `ExtensionRemoval { id, service,
data_dir_kept, site?, pool? }`. `delete_data` mặc định `false` — checkbox riêng "Xoá luôn dữ liệu",
tách khỏi nút Gỡ chính, đúng lý do kiểu này ghi: đây là thứ duy nhất một lần gỡ không trả lại được.
Kết quả vẽ đủ bốn field: service/pool đã đi cùng, domain đã thả (nếu `web-app`), và đường dữ liệu
**được giữ lại** khi có — không chỉ một toast "đã gỡ".

`extension.start`/`extension.stop` (`ExtensionTarget { id }`) — **hai verb roadmap T4.4 không nhắc
tới nhưng tồn tại thật**, và là verb đúng để gọi ở đây, không phải `service.start`/`service.stop`: dù
`ExtensionId` trùng giá trị với `ServiceId` của service đó, gọi qua namespace `extension.*` là đường
API dành riêng cho hành động này (khác `service.*`, vốn cho một service instance bất kỳ, không biết
gì về khái niệm "extension"). Trạng thái sau khi bấm vẫn đọc lại qua `service.list`/stream đã có,
không suy ra từ chính response của `start`/`stop`.

## Kiểm thử

Phần thuần, không cần daemon:

| Test | Nội dung |
| --- | --- |
| `blueprintDisposition` | `choice`/`confirm` sinh đúng `VersionAnswer`/`ScaffoldConsent` cần gửi; `blocked`/`unsupported` khoá nút Apply; các disposition còn lại không đòi hỏi gì |
| `blueprintTrustBadge` | `source`/`signature` map đúng huy hiệu; `signature: undefined` trên `builtin`/`captured` không vẽ như "đã kiểm sạch" |
| `scaffoldConsentFrom` | dựng đúng `{ command, untrusted }` từ một `PlanStep` có `action: run_scaffold` + `BlueprintPlan.trusted` |
| `extensionConsentFrom` | trích đúng bốn field `ExtensionConsent` từ một `ExtensionPlan` đã nhận, không lệch tên field |
| `webAppPermissionOrder` | `signs_in` được xếp vào khối `permissions`, không xếp cạnh `domain` |
| Job apply dùng `jobFor`/`applyJob` | job của `blueprint.apply` được `applyJob` nhận và `jobFor` tra đúng hàng — không viết state mới, chỉ test lại tích hợp với hạ tầng Pha 3 |

**Không test được bằng vitest, cần MixEngine thật:**

- Registry thật có entry `kind: "desktop-app"` đại diện MixDB không, và `ExtensionId` của nó là gì —
  điều kiện cần để đóng phần "cảnh báo tự-tham-chiếu" ở trên.
- `extension.plan` cho một `ExtensionOrigin::Path` trỏ tới một manifest cục bộ có trả đủ field như
  đường `Registry` không, hay một số field (vd. `homepage`) luôn rỗng cho nguồn `Path`.
- Tiến độ `run_scaffold` qua `GET /logs/job/{id}` có bắt đầu ngay khi job chuyển sang bước đó, hay chỉ
  có dữ liệu sau khi lệnh đã chạy xong một phần.

## Rủi ro

- **Tên `extension.*`/`ExtensionChoice`/`ExtensionChange` trùng tiền tố với `runtime.*` (Pha 3) —
  nhắc lại đúng bẫy spec Pha 3 đã tự mắc một lần.** `ExtensionsPanel.tsx` đã tồn tại ở
  [screens/Packages/ExtensionsPanel.tsx](../../apps/desktop/src/modules/mixengine/screens/Packages/ExtensionsPanel.tsx)
  — đó là PHP extension theo từng bản (`runtime.list_extensions`/`set_extension`), **không liên quan
  gì** tới màn hình Extensions (sản phẩm: Mailpit, phpMyAdmin, MixDB, `extension.*`) spec này dựng.
  Đặt tên component mới (`screens/Extensions/`) để không ai đọc lướt tưởng hai thứ là một, và một dòng
  comment ở đầu mỗi file nói rõ namespace nào.
- **`ExtensionOrigin::Path` có thể trỏ tới một extension đã cài rồi** — không field nào trên
  `ExtensionPlan` nói "cái này đã có trên máy", khác `ExtensionOffer.installed` (chỉ có ở nguồn
  Registry). Cài đè một extension đã cài qua đường Path có thể là ghi đè im lặng — chưa đo được hành
  vi thật, cần daemon thật để biết đây là refuse hay overwrite.
- **`consent` không khớp response `plan` một cách tinh vi.** Bốn field `ExtensionConsent` gửi lại
  đúng những gì `ExtensionPlan` vừa trả — nhưng nếu UI giữ state plan trong một field-by-field form
  (thay vì giữ nguyên object) và một field bị gõ nhầm khi build lại `consent`, lỗi sẽ là một
  `invalid_argument` khó đoán thay vì một refuse rõ ràng. Giữ nguyên object `ExtensionPlan` trong
  state, không destructure rồi build lại, là cách tránh lớp lỗi này.

## Quyết định

**D1 — Không có màn hình "cấu hình extension".** `extension.configure` không tồn tại trong
`mixengine-proto::rpc` lẫn bindings đã vendor — hai nguồn độc lập cùng xác nhận. Roadmap T4.4 và
`client-surface.md` mục 8 nên sửa lại: "per-extension settings" không phải một khả năng API hôm nay,
là một câu mô tả tương lai hoặc một chỗ tài liệu MixEngine tự nói trước khi xây — không phải việc của
spec này để đoán ý định, chỉ ghi nhận là không buildable.

**D2 — Install không gọi `extension.inspect`.** `extension.plan` đã trả về siêu tập những gì
`inspect` có (permissions, ports, install_dir, data_dir — `ExtensionInspection` và `ExtensionPlan` gần
như trùng field) cộng thêm phần chỉ `plan` biết (`signed`, `client`, và với path-based install: liệu
máy này thật sự cài được không). Gọi cả hai trước khi cài là đọc file manifest hai lần cho cùng một
quyết định của người dùng.

**D3 — Giữ nguyên object `ExtensionPlan` trong state của dialog Install, không destructure.** Lý do ở
Rủi ro: `consent` phải là bốn field trích thẳng từ response, không phải bốn field build lại từ một
form đã tách rời.

**D4 — Blueprints và Extensions là hai mục sidebar riêng** (không gộp như Runtimes+Packages ở Pha 3):
khác `rpc.rs` namespace, khác hình dạng dữ liệu (một cái là project template, một cái là phần mềm cài
thêm), và roadmap/`client-surface.md` đã tách hai mục 7 và 8 từ đầu — không có lý do gộp như D5 của
spec Pha 3 (nơi hai namespace cùng hình dạng RPC).

## Nợ

Ba câu đã gửi bạn hỏi bên MixEngine, chưa có trả lời — Metrics và phần "root/TLD/web server" của
Settings **chờ những câu này** trước khi viết tiếp, giữ nguyên làm phần mở của Pha 4:

1. **Per-service CPU%/RSS trên Dashboard.** `ServiceSummary` không có field nào; ba số này chỉ có
   trong `MetricsSample`/`MetricsFrame`, gắn theo `MetricsSubject` (string đóng, không phải
   `ServiceId`). Dashboard phải tự khớp `MetricsSubject` với từng service bằng tay, hay có cách nào
   khác? Và "in one read" của `client-surface.md` có nghĩa gọi `metrics.snapshot` mỗi lần render hay
   ngụ ý `service.list` *nên* có field đó?
2. **Disk usage breakdown + cleanup action** (`client-surface.md` mục 1: "disk usage broken down by
   category (runtimes, data, logs, certs) with a cleanup action"). Không method nào ở bất cứ
   namespace nào (`daemon.*`, `path.*`, `runtime.*`, `metrics.*`) tên gần với disk/cleanup/usage. Gap
   thật của API, hay tên method nằm ngoài những gì đã đối chiếu?
3. **"Default web server" trong Settings.** `root directory` (`daemon.status.home`) và `managed TLDs`
   (`domain.dns_status`/`DnsStatus.wildcards`) đọc được; "default web server" (Caddy/Nginx, ADR 0004)
   thì không — `FrontEndServer` chỉ xuất hiện trong `RecipeAddition` (soạn fragment), không phải một
   trạng thái máy đọc được qua bất cứ method nào đã tìm thấy. Có `settings.*` nào đọc/ghi lựa chọn
   này của một home không?

Khi có trả lời, Metrics và Settings là một spec riêng, nối tiếp spec này — không sửa lại spec này, vì
Blueprints/Extensions không phụ thuộc câu trả lời nào ở trên.
