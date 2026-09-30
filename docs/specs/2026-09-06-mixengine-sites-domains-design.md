---
status: implemented
date: 2026-09-06
---

# Sites, Domains & TLS: màn hình thứ hai và thứ ba của module `mixengine`

Ngày 2026-09-06. Pha 2 của [roadmap/mixengine-module.md](../roadmap/desktop/mixengine-module.md).

## Mục tiêu

- Một sidebar thật trong tab `mixengine`, chuyển được giữa ba màn hình: Dashboard (Pha 1, không đổi),
  Sites, Domains & TLS.
- **Sites**: liệt kê mọi site, tạo mới, sửa, mở doc root / URL / terminal của nó.
- **Chia sẻ site ra LAN**: bật, tắt, thấy nó tự hết hạn hoặc tự rớt mạng và đúng vì sao — đây là lý do
  T2.4 tồn tại: `mix` không có cách nào hiện được thông báo này.
- **Domains & TLS**: bảng chẩn đoán domain, trạng thái CA (kho hệ thống *và* trình duyệt, hai câu trả
  lời chứ không phải một), chứng chỉ từng site trong một bảng.

## Phi mục tiêu

- Runtimes, Services (settings/limits/credentials), Logs — Pha 3. Blueprints, Extensions, Settings,
  Metrics — Pha 4.
- Không dựng màn hình quản lý **Projects**. `site.create` cần một `project`, nhưng Projects không nằm
  trong 9 màn hình của `client-surface.md` và không có dòng nào trong bảng ánh xạ màn hình → pha của
  roadmap. Mục 2 dưới đây nói rõ Pha này định thu hẹp việc đó lại thế nào — xem **Câu hỏi 1**.
- Không `cert.list`, `cert.renew`, `cert.ca_install` — cả ba bị daemon từ chối có lý do (roadmap đã
  ghi), the standalone client không gọi.
- Không tự chọn network interface khi máy có nhiều ứng viên — daemon từ chối, the standalone client chỉ hiện lại lý do
  daemon đưa ra, không tự đoán ứng viên nào đúng.
- Không `site.delete` — roadmap T2.2 chỉ nói "tạo và sửa". Xem **Câu hỏi 2**.

## Hiện trạng (sau Pha 1)

Ba thứ Pha 1 để lại và Pha 2 dựng lên trên:

- **Một screen, không sidebar.** `MixEngineTab.tsx` hiện chỉ có `screen === "dashboard" && <Dashboard />`
  — không có bộ chuyển màn hình nào cả, vì Pha 1 chỉ có một màn hình để chuyển tới.
  `tabState.ts` khai `screen: "dashboard"` là literal type duy nhất.
- **Khuôn lệnh đã có, và nó rất mỏng.** `api.ts` là nơi duy nhất gọi `invoke()`; mỗi hàm bọc một
  lệnh Tauri, lệnh Tauri gọi `rpc::call(method, params)`, và phần lớn trả thẳng `serde_json::Value`
  — Rust không giải vào struct riêng, vì "types là thứ đi mượn" (xem
  [commands.rs](../../apps/desktop/src-tauri/src/modules/mixengine/commands.rs)). Lỗi đã có ánh xạ chung
  (`error.mixengineRefused` mang `code`/`message`/`hint`) cho **mọi** RPC thất bại — Pha 2 không cần
  thêm mã lỗi nào mới, chỉ thêm lệnh.
- **`bindings/` đã vendor toàn bộ, không phải theo pha.** Cả `Site*`, `Domain*`, `Ca*`, `Cert*`,
  `Browser*`, `Project*` (296 file) đã nằm sẵn trong `src/modules/mixengine/api/types/` từ tarball Pha
  1 vendor. Pha 2 không cần chạy `npm run bindings` lại, trừ khi muốn lên phiên bản mới của MixEngine.

## 1. Điều hướng: sidebar thật, lần đầu tiên

Đây là pha đầu tiên có nhiều hơn một màn hình, nên là pha đầu tiên phải xây cái sidebar mà cả roadmap
lẫn spec Pha 1 đều nhắc tới nhưng chưa ai dựng.

- **`tabState.ts`**: `screen: "dashboard" | "sites" | "domains"`. Vẫn ids-only — không site nào, không
  domain nào được nhớ ở đây, chỉ tên màn hình đang mở, đúng luật đã đặt ở Pha 1.
- **`components/Sidebar/`** (mới): danh sách 9 mục cố định của `client-surface.md`; 3 mục bật
  (Dashboard, Sites, Domains & TLS), 6 mục còn lại xám và không bấm được — **không ẩn hẳn**, cùng luật
  Pha 1 đã theo cho Dashboard/Services: một mục xám nói "sắp có", một mục biến mất thì không nói gì cả
  và không ai biết còn 6 màn hình nữa đang tới.
- **`MixEngineTab.tsx`**: sau cổng ba trạng thái (không đổi), render `<Sidebar>` + màn hình tương ứng
  với `screen`, thay vì so sánh trực tiếp một chữ.

## 2. Sites — T2.1, T2.2

### Liệt kê

`site.list` (`SiteListQuery { project? }` → `SiteList { sites: SiteSummary[] }`). Không lọc theo
project ở Pha này — gọi không tham số, thấy mọi site trong home.

Bảng: domain (chính), owner, kind, https, state, sharing (huy hiệu nếu đang chia sẻ — T2.3).

**`owner` quyết định những nút nào bật**, đọc thẳng từ `SiteOwner` (đã là dữ liệu report, không phải
suy đoán):

- `{ type: "project", name }` — sửa được bình thường.
- `{ type: "extension", id }` — nút Sửa và Xoá tắt, thay bằng một dòng
  *"của extension `id` — gỡ bằng lệnh gỡ extension đó"*. Daemon vẫn là nơi quyết định thật (một
  `site.update` gửi thẳng vào site này sẽ bị từ chối dù UI cho phép), nhưng để nút bấm luôn hỏng là
  hứa một hành động không giữ được.

### Xem chi tiết, trước khi sửa

`site.show` (`SiteQuery { site: { domain } }` → `SiteDetail`). Đây là chỗ ba việc chỉ lookup mới trả
lời được, và form Sửa dựng trên nó chứ không dựng trên `SiteSummary`:

- `doc_root_full` — đường tuyệt đối, **daemon đã ghép `root` (thư mục project) với `doc_root`**, the standalone client
  không tự ghép hai chuỗi lại. "Mở trong file manager" dùng thẳng field này.
- `doc_root_exists` — báo cáo, không từ chối (nguyên văn spec bên đó): thư mục `public/` sinh ra từ
  `npm run build` chưa chạy vẫn là một site hợp lệ.
- `pool` (`SitePool { declared?, resolved? }`) cho site `php-fpm` — hai câu trả lời khác nhau khi
  phiên bản mặc định của project đổi mà site không đổi theo; vẽ cả hai, không chỉ một.
- `services: SiteServiceLink[]` — mỗi service kèm trạng thái sẵn, khỏi gọi `service.list` lần hai.

### Tạo và sửa

`site.create` (`SiteCreate`) / `site.update` (`SiteUpdate`). Form:

- **Project** — bắt buộc lúc tạo, không đổi được lúc sửa. Xem Câu hỏi 1 cho việc chọn nó ở đâu ra.
- **Domains** — danh sách có thứ tự, đầu danh sách là domain chính; thêm/bớt/kéo thả để đổi thứ tự.
  `SiteUpdate.domains` **thay thế toàn bộ danh sách**, không merge — gửi cả danh sách hiện tại cộng
  thay đổi, không gửi mỗi domain mới.
- **Doc root** — text field + nút duyệt thư mục (dialog file hệ thống the standalone client đã có ở module khác, tái
  dùng chứ không viết lại).
- **Kind** — `php-fpm` (chọn pool từ `service.list` lọc theo runtime PHP), `static` (không thêm field
  nào), `reverse-proxy` (một URL `http`/`https` có host), `node-app` (một cổng loopback).
- **Services** — multi-select `ServiceId` từ `service.list`.
- **HTTPS** — công tắc. Chỉ là khai báo ở Pha này — MixEngine nói rõ "chưa có gì tự hành động theo cờ
  này hôm nay"; nó chờ `cert.issue` ở mục 4 làm phần việc thật.
- **`accept_risky_tld`** — checkbox, chỉ hiện khi danh sách domain có một tên kết thúc bằng `.local`.
- **State** — `enabled`/`disabled`, sửa qua `site.update { state }`, không phải qua service action nào.

Mở doc root / mở URL / mở terminal tại site: ba khả năng module `tools` đã có sẵn ở nơi khác trong
The standalone client (roadmap nói thẳng "the standalone client đã có sẵn cả ba đường đó") — Pha này nối chúng vào nút trên hàng site,
không viết lại file-manager hay terminal opener nào mới.

## 3. Chia sẻ LAN — T2.3, T2.4

`site.share` (`SiteShare { site, interface?, for_seconds? }` → trả `SiteSharing`). `site.unshare`
(cùng dạng `{ site }`).

- **Không tự chọn interface.** Bỏ trống `interface` là đường thường (máy chỉ có một candidate). Máy có
  nhiều hơn một, daemon từ chối và **nêu tên trong `error.hint`** — `ErrorData` chỉ có `code` và
  `hint` dạng câu, không có một field `candidates` có cấu trúc. Nên: hiện `hint` nguyên văn trong
  banner lỗi (như mọi lỗi khác), và ô "interface" trên form Share luôn là một text field gõ tay chứ
  không phải dropdown tự điền — gõ đúng tên đọc được từ `hint` rồi bấm lại. Không parse `hint` thành
  danh sách: đó là nghiệp vụ, và nó ở phía daemon.
- **`for_seconds`** đặt hạn; `SiteSharing.until` mang hạn về để vẽ đếm ngược. Không đặt hạn là chia sẻ
  không tự hết.
- **`site_sharing_changed`** trên stream sự kiện (đã mở từ Pha 1 cho `service_state_changed` — cùng
  một `api.watch`, thêm một nhánh `type` mới) mang `sharing: SiteSharing | null` và `because:
  SharingChange` (`requested` / `expired` / `network_changed { was, now? }`). Khi `sharing` là `null`,
  vẽ đúng một câu theo `because` — đây chính là T2.4, chỗ roadmap gọi là "chỗ duy nhất `mix` là client
  yếu hơn": với CLI lý do nằm trong log không ai đọc, ở đây nó phải là một dòng thông báo thấy được
  ngay.

## 4. Domains & TLS — T2.5, T2.6, T2.7

Một màn hình, ba khối, dựng trên bốn method.

### Chẩn đoán domain — T2.5

`domain.dns_status` (`DomainStatusQuery { domain? }` → `DomainStatusReport { domains: DomainStatus[] }`)
**là cả `domain.list` lẫn diagnostic của từng tên** — roadmap gọi chúng bằng hai cái tên nhưng đây
chỉ là một method: bỏ trống `domain` để thấy mọi tên, truyền một tên để refresh đúng hàng đó sau khi
sửa. `domain.add` (`DomainAdd { site, domain, accept_risky_tld }`) / `domain.remove`
(`DomainRemove { domain }` — không cần nêu site, tên tự xác định site của nó, chỉ the standalone client không được tự
suy ra ngược lại).

Bảng: domain, site khai nó (hoặc trống — "tên này chưa ai khai"), `hosts_entry`, `wildcard`,
`server_answers`, `resolves_to`, và một cột `because` khi có — **một câu, không phải một mã**, vẽ
nguyên văn.

### CA — T2.6

`cert.ca_status` (không tham số) → `CaStatus`. Struct phẳng hoá một enum ba nhánh
(`absent`/`present`/`unusable`) cộng hai field độc lập với nhánh đó:

- `trust: Trust` — máy này (kho hệ thống) có tin không.
- `browsers: Browsers` — Firefox/Chrome NSS database nào có, mỗi cái kèm đường dẫn.

**Vẽ hai hàng, không gộp làm một tick xanh** — đúng câu roadmap cảnh báo: một dấu tick chung dễ nói
dối về trình duyệt đang hiện khoá đỏ.

Nút "Sửa" cho nửa `browsers` gọi `daemon.doctor_repair` (`DoctorRepair { grant }`), **không qua luồng
elevation** — cài vào NSS database là việc trong quyền tài khoản này, không cần quyền quản trị. Đây là
chỗ Pha 2 mượn trước đúng hai method của màn hình Settings/Doctor đầy đủ (`daemon.doctor` /
`daemon.doctor_repair`), Pha 4 dựng màn hình doctor thật lên trên cùng hai method này. Xem
**Câu hỏi 3** — cần xác nhận với daemon thật là sửa CA không đẩy gì vào hàng đợi elevation trước khi
khoá `grant: true` cứng vào nút bấm.

### Chứng chỉ từng site — T2.7

`cert.issue` (`CertIssue { site? }` → `CertIssueReport { sites: SiteCertOutcome[] }`). Roadmap chọn
đúng method này để **vừa vẽ bảng vừa cấp lại**, không dùng `cert.status` (tồn tại trong hợp đồng,
nhưng không nằm trong phạm vi T2.7):

- Gọi không `site` → mọi site có khai HTTPS, dùng để vẽ cả bảng bằng một call.
- Mỗi hàng: `domain`, `outcome` (`issued`/`reused`/`not_wanted`/`refused`, hai cái sau kèm `because`),
  `state: CertState` — khi `present`, `state.cert.sans` là những tên nó phủ và
  `state.cert.days_left` là số ngày còn lại. Không cần gọi gì thêm để có hai con số roadmap nhắc tới.
- Nút "Cấp lại" trên một hàng gọi lại đúng method này với `{ site: { domain } }` — idempotent, không
  bật prompt.

## 5. Lệnh Tauri mới

Nối dài `commands.rs` theo đúng khuôn Pha 1: một lệnh mỏng, không nghiệp vụ, trả `Value`.

| Lệnh | RPC method |
| --- | --- |
| `mixengine_sites` | `site.list` |
| `mixengine_site` | `site.show` |
| `mixengine_site_create` | `site.create` |
| `mixengine_site_update` | `site.update` |
| `mixengine_site_share` | `site.share` |
| `mixengine_site_unshare` | `site.unshare` |
| `mixengine_domains` | `domain.dns_status` |
| `mixengine_domain_add` | `domain.add` |
| `mixengine_domain_remove` | `domain.remove` |
| `mixengine_ca_status` | `cert.ca_status` |
| `mixengine_ca_repair` | `daemon.doctor_repair` |
| `mixengine_certs` | `cert.issue` |

**Quyết định cần chốt: tham số truyền thẳng `Value` thay vì scalar riêng từng field.** Mọi lệnh Pha 1
nhận scalar (`id: String, action: String`) vì params của chúng chỉ có một hai field. `site.create` /
`site.update` / `site.share` có 4-8 field tuỳ chọn mỗi cái — khai một struct Rust cho từng cái là chép
tay thứ hai của một hợp đồng frontend đã gõ đúng qua `SiteCreate`/`SiteUpdate`/`SiteShare`, và lệch
với "types là thứ đi mượn" đã chốt ở Pha 1. Đề xuất: các lệnh có params phức tạp nhận thẳng
`params: serde_json::Value` từ `invoke()` và chuyển tiếp nguyên xi vào `rpc::call`; phía TypeScript
trong `api.ts` là nơi chịu trách nhiệm gõ kiểu đúng bằng `SiteCreate` v.v. trước khi gọi. Cần đồng ý
trước khi viết code — đây là điểm lệch nhỏ so với khuôn Pha 1 chứ không phải tiếp tục y nguyên nó.

Không thêm mã lỗi nào vào `error.rs`: bốn mã Pha 1 đã thêm
(`mixengineUnreachable`/`mixengineRefused`/`mixengineProtocol`/`mixenginePipeOwner`) phủ hết mọi cách
RPC Pha 2 hỏng.

## 6. Kiểm thử

| Test | Nội dung |
| --- | --- |
| `tabState` | `parseMixEngineTabState` nhận cả ba `screen`, từ chối cái thứ tư |
| `SiteOwner` → quyền sửa | hàm thuần `canEditSite(owner)` — `project` true, `extension` false |
| Sự kiện `site_sharing_changed` | `sharing: null` vẽ đúng câu cho cả ba `because.kind`; `sharing` có
  giá trị cập nhật đúng hàng |
| `CaStatus` | vẽ đúng hai hàng độc lập cho mọi tổ hợp `trust`/`browsers`, kể cả khi CA `absent` |
| `cert.issue` không site | hàm dựng bảng từ `CertIssueReport` đọc đúng `sans`/`days_left` khi
  `state.state === "present"`, không throw khi `absent`/`unusable` |
| `DomainAdd`/`DomainRemove` | payload đúng field, `accept_risky_tld` mặc định `false` |

Không test thuần được, cần daemon thật (giống ghi chú Pha 1): interface LAN thật có nhiều hơn một
candidate hay không tuỳ máy dựng; `daemon.doctor_repair` có đẩy gì vào elevation queue cho CA hay
không (Câu hỏi 3); `cert.issue` không `site` trên một home chưa `.test` nào issue lần nào.

## Câu hỏi mở

1. **Chọn project lúc tạo site, từ đâu ra.** `site.create` bắt buộc một `project: ProjectRef`, nhưng
   Projects không phải một trong 9 màn hình và không có dòng nào trong bảng ánh xạ màn hình → pha —
   không phase nào của roadmap này dựng một màn hình Projects. Ba hướng: (a) dropdown gọi `project.list`
   thẳng, không màn hình Projects riêng, một máy chưa có project nào thì nút Tạo site vô dụng cho tới
   khi ai đó `mix project add` bằng tay; (b) như (a) cộng một form mini "tạo project mới" ngay trong
   hộp thoại tạo site (`project.create` cần `root` tuyệt đối có thật, tối thiểu cũng là một dialog chọn
   thư mục); (c) coi đây là lỗ hổng phạm vi và chặn hẳn nút Tạo site lại, chỉ để Sửa hoạt động ở Pha
   này. Cần quyết trước khi viết form Sites.
2. **`site.delete` có vào Pha 2 không.** Hợp đồng có method này (`SiteRemoval`), nhưng T2.2 trong
   roadmap chỉ nói "tạo và sửa". Thêm nút Xoá là một buổi chiều, nhưng là việc ngoài đúng câu chữ
   roadmap đã chốt — hỏi trước khi thêm.
3. **`daemon.doctor_repair` cho CA có bao giờ cần elevation không.** T2.6 khẳng định "không bật
   prompt", nhưng `DoctorRepair.grant` tồn tại chính vì một số sửa **cần** prompt. Nếu sửa CA luôn
   không cần, khoá `grant: true` cứng là an toàn; nếu có trường hợp cần, nút này phải đổi thành luồng
   hai bước như `elevation.*` ở Dashboard. Chỉ đo được với MixEngine thật.
4. **Tham số `Value` thay vì struct** (mục 5) — cần đồng ý trước khi viết `commands.rs`.
