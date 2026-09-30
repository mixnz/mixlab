---
status: implemented
date: 2026-08-28
---

# Module Tools — Giai đoạn 4: kết nối và hạ tầng

Ngày: 2026-08-28

Spec con của [module Tools](2026-08-28-tools-module-design.md), tiếp sau
[giai đoạn 3](2026-08-28-tools-phase-3-design.md). Ba giai đoạn đầu đã ship và module có 12 tool.
Đây là giai đoạn cuối trong bản phác của spec mẹ, và là giai đoạn **duy nhất chạm Rust**, **duy
nhất cần lưu trữ**. Nó để cuối vì rủi ro cao nhất, không phải vì kém giá trị.

Tiêu chí nhận tool không đổi:

> Chỉ nhận tool nằm trên đường đi của một dev đang làm việc với DB, API hoặc server.

## Mục tiêu

Sau giai đoạn này:

- Module có **15 tool**, và nhóm `infra` từ 1 lên 4 mục — thành nhóm đông thứ hai.
- Ba tool mới chạy được: xem cổng đang nghe, cheatsheet có tham số, và đọc/ghi chuỗi kết nối.
- Module có **lệnh Rust đầu tiên**, và nó chỉ đọc.
- Module có **store đầu tiên giữ nội dung người dùng viết** — snippet, và chỉ snippet.
- **Không dependency mới**, cả JavaScript lẫn Rust.

## Phi mục tiêu

Bốn cái đầu thừa hưởng từ spec mẹ và giai đoạn này không xin ngoại lệ nào:

- **Không chạy gì cả.** Tool cổng in ra lệnh kill để chép; nó không giết tiến trình nào. Đây là
  ranh giới an toàn của cả module, và giai đoạn này là chỗ duy nhất nó bị thử.
- **Không kết nối thử.** Tool chuỗi kết nối tách và ghép chuỗi; nó không mở kết nối nào để xem
  chuỗi đó có đúng không.
- **Không bắc cầu sang module khác.** Không có nút "tạo saved connection từ chuỗi này" hay "mở
  lệnh này ở tab Terminal".
- **Không lưu nội dung người dùng gõ** — trừ đúng một ngoại lệ mà spec mẹ đã cho phép: cheatsheet
  lưu **template**, không bao giờ lưu giá trị tham số. Xem 3.4.

Thêm ba cái của riêng giai đoạn này:

- **Không quét máy khác.** Chỉ cổng của máy đang chạy the standalone client. Xem 2.1.
- **Không lấy đường dẫn tiến trình.** Xem 2.4.
- **Không bịa JDBC cho MongoDB và Redis.** Xem 4.3.

## Hiện trạng

Ba điều kiểm được trong code, và cả ba đều quyết định thiết kế bên dưới:

1. **Tiền tố `tools_` đã có chủ.** `db::commands::tools::tools_status`, `tools_ready`,
   `tools_downloadable`, `tools_install`, `tools_uninstall`, `tools_set_path` — sáu lệnh quản
   chương trình dump/restore của module db. Cùng loại va chạm mà nhóm i18n `tools`/`toolbox` gặp ở
   giai đoạn 1, nhưng lần này **không có lưới nào bắt**: Tauri không phàn nàn gì, chỉ là hai họ
   lệnh khác chủ nằm lẫn nhau trong một danh sách. Xem mục 0.
2. **`toolsEn.error` chưa được nối vào `dicts.ts`.** Nhóm `error` được gộp tay, và dòng gộp hiện
   chỉ có `shared`, `db`, `rest`, `terminal` — vì tới giờ module Tools chưa phát lỗi nào từ Rust.
   Giai đoạn này là lần đầu nó phát.
3. **Module chưa có nửa backend nào.** `src-tauri/src/modules/` có ba thư mục: `db`, `rest`,
   `terminal`. Giai đoạn này thêm cái thứ tư.

Và những gì có sẵn để dựa vào:

| Chỗ | Dùng để làm gì |
| --- | --- |
| `src-tauri/src/error.rs` | `AppError { code, params }` — mã lỗi đi kèm tham số, frontend dịch |
| `src-tauri/src/modules/terminal/commands.rs` | Pattern một lệnh chỉ đọc chạy trên thread blocking |
| `src/core/jsonStore.ts` | `createStore` + `jsonFile` — module đã dùng cho `tools-workspace.json` |
| `src/modules/rest/requests.ts` + `requestsStore.ts` | Pattern tách thao tác thuần khỏi phần nối đĩa |
| `tools/tools/env/env.ts` | `EnvPair`, `toEnv`, `toDockerArgs` — mục 4.4 dùng lại |
| `tools/registry.ts` | Ba dòng nữa |

## 0. Dọn tên lệnh trước

Sáu lệnh của module db đổi tiền tố:

| Cũ | Mới |
| --- | --- |
| `tools_status` | `dumptools_status` |
| `tools_ready` | `dumptools_ready` |
| `tools_downloadable` | `dumptools_downloadable` |
| `tools_install` | `dumptools_install` |
| `tools_uninstall` | `dumptools_uninstall` |
| `tools_set_path` | `dumptools_set_path` |

`dumptools` chứ không phải `db_tools`: tên nói **chúng là gì** — chương trình dump và restore —
chứ không chỉ nói ai sở hữu, mà quyền sở hữu thì đã nằm sẵn trong đường dẫn `db::commands::tools::`
rồi. Chính file đó tự mô tả mình ở dòng đầu là "the dump and restore tools".

Phạm vi hẹp và đếm được: sáu tên hàm trong `src-tauri/src/modules/db/commands/tools.rs`, sáu dòng
trong `generate_handler!`, sáu chuỗi `invoke` trong `src/modules/db/tools.ts`. Không có consumer nào
khác — `grep` cho từng tên chỉ ra đúng ba chỗ đó.

**Nhóm i18n `toolbox` không đổi.** Đổi nó về `tools` là sửa hơn trăm khoá ở hai từ điển cộng mọi
Panel, và không có gì người dùng thấy được. Cái tên `toolbox` đã trả xong giá của nó ở giai đoạn 1.

## 1. Ba tool

| Tool | id | Nhóm | Nội dung |
| --- | --- | --- | --- |
| Cổng đang nghe | `ports` | `infra` | Bảng cổng → PID → tiến trình, cộng lệnh kill theo OS |
| Cheatsheet | `cheatsheet` | `infra` | Snippet có tham số: bộ sẵn có cộng của người dùng |
| Chuỗi kết nối | `connection` | `infra` | URI ⇄ các trường ⇄ JDBC / `.env` / docker |

Cả ba vào `infra`, nhóm mà giai đoạn 3 vừa mở hàng bằng tool Biến môi trường.

## 2. Cổng đang nghe (`ports`)

### 2.1 Lệnh `tools_listening_ports`

Tên là `listening_ports` chứ không phải `scan_port` như bản phác của spec mẹ: tool liệt kê **cả
bảng** cổng đang nghe chứ không tra từng cổng một, và `scan_port` sẽ mô tả sai việc nó làm. Mở tool
ra là thấy ngay máy này đang mở những gì, có ô lọc theo số cổng cho người đã biết mình tìm gì.

Chỉ máy đang chạy the standalone client. Quét máy khác đòi một đường đi qua SSH mà tool này không có, và nó là một
thiết kế riêng nếu có ai cần.

```rust
pub struct ListeningPort {
    pub port: u16,
    /// Địa chỉ đang nghe: `0.0.0.0`, `127.0.0.1`, `::`. Phân biệt "mở ra ngoài" với "chỉ localhost".
    pub address: String,
    pub pid: u32,
    /// `None` khi tra được cổng nhưng không tra được tên tiến trình — thường là do thiếu quyền.
    pub process: Option<String>,
}
```

### 2.2 Tách "chạy lệnh" khỏi "đọc kết quả"

Điểm thiết kế quan trọng nhất của mục này, và là lý do phần Rust này test được.

Chạy `netstat`/`ss`/`lsof` là I/O: nó phụ thuộc máy, phụ thuộc quyền, và không có cách nào test
trong CI. Đọc output của chúng là hàm thuần `&str → Vec<ListeningPort>`, và **đó là chỗ mọi lỗi sẽ
nằm**: ba nền tảng, ba định dạng, mỗi cái có một kiểu dòng lạ mà người viết không nghĩ tới.

Nên hai nửa nằm ở hai chỗ: `commands.rs` chạy lệnh và không làm gì khác, `ports.rs` đọc chuỗi và
không biết `Command` là gì. Bộ test là output thật đã bắt lại, dán nguyên vào file test làm fixture.
Đây là lần đầu module này có test Rust, và nó đúng cùng triết lý với phía frontend: thứ thuần thì
test, thứ chạm hệ điều hành thì không.

### 2.3 Ba nền tảng

| Nền tảng | Lệnh | Phần khó |
| --- | --- | --- |
| Windows | `netstat -ano`, rồi `tasklist /FO CSV /NH` để tra tên theo PID | Bảng canh cột, có cả dòng IPv6 dạng `[::]:445`; lọc theo trạng thái `LISTENING` |
| Linux | `ss -lntp` | `users:(("nginx",pid=123,fd=6))` — tên và PID nằm lồng trong ngoặc |
| macOS | `lsof -nP -iTCP -sTCP:LISTEN -Fpcn` | Dạng field: mỗi dòng một trường, dòng `p`/`c` mở đầu một tiến trình rồi các dòng `n` thuộc về nó |

**Không dùng `Get-NetTCPConnection` trên Windows**, dù nó ra dữ liệu sạch hơn: nó cần PowerShell, và
execution policy trên một máy công ty có thể chặn. `netstat` thì có ở mọi bản Windows và không hỏi
gì.

Và **`netstat -ano` chứ không phải `netstat -ano -p TCP`**, dù cờ `-p TCP` trông đúng hơn: đã đo
trên máy thật, `-p TCP` **lọc mất toàn bộ IPv6** — một service chỉ nghe trên `[::]` sẽ biến mất khỏi
bảng mà không có dấu hiệu gì. IPv6 nằm dưới `-p TCPv6`, và chạy hai lệnh để ghép lại thì tốn hơn là
lọc trong bộ đọc. Bỏ `-p` thì output có thêm UDP, nhưng phân biệt được bằng số cột — cũng đã đo:

| Loại dòng | Số cột | Cột |
| --- | --- | --- |
| TCP | 5 | `Proto` `Local` `Foreign` `State` `PID` |
| UDP | 4 | `Proto` `Local` `Foreign` `PID` — **không có cột trạng thái** |

Nên luật của bộ đọc là: đúng 5 cột, cột đầu là `TCP`, cột thứ tư là `LISTENING`. UDP tự rụng vì
thiếu cột, và TCP đang `ESTABLISHED` tự rụng vì sai trạng thái.

`ss -lntp` không dùng cờ `-H`: cờ đó chỉ có ở `iproute2` đời mới, và bỏ dòng tiêu đề trong bộ đọc
thì rẻ hơn là đòi hỏi phiên bản. Máy Linux không có `ss` thì lùi về `lsof` — cùng bộ đọc với macOS.

Không tra được tên tiến trình không phải lỗi: `ss` và `lsof` chỉ thấy tiến trình của người dùng khác
khi chạy với quyền cao, và một bảng có cổng với PID mà thiếu tên vẫn trả lời được câu hỏi người ta
đang hỏi. Đó là lý do `process` là `Option`.

**Fixture của Windows bắt được ngay trên máy phát triển. Fixture Linux và macOS viết theo định dạng
tài liệu hoá, và cần một lần đối chiếu trên máy thật** trước khi giai đoạn được coi là xong — bộ
đọc đúng với một fixture do chính người viết bịa ra là bộ đọc chưa được kiểm chứng.

### 2.4 Không lấy đường dẫn tiến trình

Bản phác của spec mẹ nói lệnh trả về "PID, tên tiến trình, đường dẫn". Bỏ đường dẫn.

Trên ba nền tảng nó là ba cách khác hẳn nhau — `/proc/<pid>/exe`, `ps -p <pid> -o comm=`,
`Get-Process | Select Path` — nên nó gần gấp đôi phần việc backend, và cái cuối lại kéo PowerShell
vào đúng chỗ 2.3 vừa tránh. Trong bốn trường thì đây là trường ít được nhìn nhất: biết cổng 3000 do
`node` giữ là đã đủ để quyết định giết hay không.

Thêm sau được, và lúc đó nó là một trường nữa trong `ListeningPort` chứ không phải một thiết kế
khác.

### 2.5 Lệnh kill

Hàm thuần phía frontend, không dính gì tới backend:

| OS | Theo PID | Theo cổng |
| --- | --- | --- |
| macOS / Linux | `kill -9 <pid>` | `lsof -ti:<port> \| xargs kill -9` |
| Windows | `taskkill /PID <pid> /F` | `for /f "tokens=5" %a in ('netstat -ano ^\| findstr :<port>') do taskkill /PID %a /F` |

Ô chọn OS mặc định theo máy đang chạy nhưng **đổi tay được**, và đó là chủ ý của spec mẹ: người ngồi
Windows thường xuyên cần lệnh kill cho một server Linux đang mở ở tab Terminal bên cạnh. Mặc định
theo máy là để lần dùng phổ biến nhất không phải bấm gì; đổi tay được là để lần dùng kia làm được.

## 3. Cheatsheet (`cheatsheet`)

Tool yếu nhất của giai đoạn so với tiêu chí nhận tool, và đáng ghi lại vì sao nó vẫn được nhận:
**phần điền tham số là chức năng, không phải chỗ chứa.** Một danh sách lệnh để chép là một file ghi
chú; một danh sách lệnh đọc ra `{{tham số}}` rồi dựng lại lệnh đã điền là một tool. Nếu về sau phần
tham số bị bỏ đi thì tool này cũng nên bị bỏ theo.

### 3.1 Template và tham số

```ts
export interface Snippet {
  id: string;
  title: string;
  /** Nhóm để xếp danh sách: "mysql", "postgres", "docker", "ssh"… Chuỗi tự do. */
  group: string;
  /** Lệnh, với tham số viết `{{tên}}`. */
  template: string;
}

/** Tên các tham số, theo thứ tự xuất hiện lần đầu, không lặp. */
export function paramsOf(template: string): string[];

/** Thay `{{tên}}` bằng giá trị. Tên không có giá trị thì giữ nguyên `{{tên}}` — một ô chưa điền
 *  phải nhìn thấy được trong đầu ra, chứ không biến mất thành khoảng trắng. */
export function fill(template: string, values: Record<string, string>): string;
```

Panel đọc `paramsOf`, hiện ra đúng bấy nhiêu ô nhập, và in kết quả `fill` vào một `CopyField`.

### 3.2 Tool không bọc ngoặc hộ

Một mật khẩu có dấu cách dán vào `{{password}}` sẽ làm hỏng lệnh, và câu hỏi tự nhiên là tool có nên
tự bọc ngoặc không. **Không.**

Không phải mọi `{{tham số}}` đều đứng ở vị trí một đối số shell: có cái nằm giữa một URL, có cái nằm
trong một chuỗi đã được bọc sẵn. Bọc hộ ở chỗ template đã bọc rồi là bọc hai lần, và nó hỏng theo
một cách khó thấy hơn hẳn cách nó đang hỏng. Người viết template là người biết chỗ nào cần ngoặc,
và bộ sẵn có tự đặt ngoặc ở nơi cần — `-p'{{password}}'`, không phải `-p{{password}}`.

Cùng một luật với tool Chuyển đổi ở giai đoạn 3: tool không đoán hộ những gì nó không đủ thông tin
để đoán.

### 3.3 Bộ sẵn có nằm trong code

Bộ sẵn có là một hằng số trong `builtin.ts`, **không đi qua store**. Hệ quả là hai chiều và cả hai
đều đúng: nâng bản the standalone client thì bộ sẵn có được cập nhật theo, và thứ người dùng tự viết thì không bao
giờ bị một bản nâng cấp ghi đè.

Snippet sẵn có không sửa và không xoá được. Muốn một bản khác đi thì thêm một snippet của mình —
đơn giản hơn hẳn việc dựng khái niệm "bản sẵn có đã bị người dùng sửa", thứ phải trả lời câu hỏi
"bản nâng cấp đổi snippet này thì sao" mà không có câu trả lời nào dễ chịu.

Chừng mười lăm mục, và mỗi mục phải qua được tiêu chí nhận tool của module: `mysqldump`,
`mysql` restore, `pg_dump`, `pg_restore`, `psql`, `mongodump`, `mongorestore`, `redis-cli`,
`docker run` cho MySQL và PostgreSQL, `docker logs`, `docker system prune`, `systemctl status`,
`journalctl -u`, SSH tunnel `-L`, và `scp`.

### 3.4 Store

`tools-snippets.json`, theo đúng pattern `requests.ts` + `requestsStore.ts` của module rest: thao
tác thuần — thêm, sửa, xoá — nằm trong `snippets.ts` và có test; phần nối vào đĩa nằm trong
`snippetsStore.ts` và không có test.

**Store giữ template, không bao giờ giữ giá trị tham số.** Giá trị sống trong state của tab rồi mất
khi đóng tab, như mọi tool khác trong module — và đó là chỗ mật khẩu đi qua. Đây là điều làm ngoại
lệ "cheatsheet được lưu" của spec mẹ vẫn nằm trong luật "không lưu nội dung người dùng gõ": thứ
được lưu là cái khuôn, không phải cái đổ vào khuôn.

## 4. Chuỗi kết nối (`connection`)

Một ô dán URI vào, một bảng các trường sửa được, và bốn dạng đầu ra. Sửa ở đâu cũng được và các
dạng còn lại theo kịp.

### 4.1 Các trường

```ts
export type DbKind = "mysql" | "postgres" | "mongodb" | "redis";

export interface ConnectionFields {
  kind: DbKind;
  host: string;
  /** Chuỗi chứ không phải số: ô rỗng là "dùng mặc định", và `0` không phải cách nói điều đó. */
  port: string;
  user: string;
  password: string;
  database: string;
  params: { key: string; value: string }[];
}

export function parseConnectionString(text: string): ConnectionFields | null;
```

Cổng mặc định khi ô rỗng: MySQL 3306, PostgreSQL 5432, MongoDB 27017, Redis 6379.

Scheme nhận được: `mysql://`, `postgresql://` và `postgres://`, `mongodb://` và `mongodb+srv://`,
`redis://` và `rediss://`.

Hai chỗ lệch giữa các loại, cả hai đều phải đúng chứ không được gộp:

- **`mongodb+srv://` không có cổng.** Bản ghi SRV của DNS mới là thứ nói cổng, nên một URI `+srv`
  mang cổng là một URI sai. Chọn `+srv` thì ô cổng bị khoá và bỏ trống.
- **Redis đánh số database chứ không đặt tên.** `redis://host:6379/0` — phần path là một con số,
  không phải một cái tên.

### 4.2 Percent-encoding của mật khẩu

Chỗ tinh của tool này. Đã đo trên `URL` thật thay vì suy đoán, và kết quả không như trực giác:

| Ký tự trong mật khẩu, chưa encode | `new URL("mysql://user:p⟨ch⟩ss@host:3306/db")` |
| --- | --- |
| `@` | Parse **đúng** — host `host`, mật khẩu `p%40ss`. Luật là cắt ở dấu `@` **cuối cùng**, không phải dấu đầu |
| `:` | Parse **đúng** — dấu `:` thứ hai trở đi thuộc về mật khẩu |
| `/` `?` `#` | **Ném `Invalid URL`** — phần authority kết thúc ở ký tự đầu tiên trong ba cái này |

Nên nguy hiểm **không** nằm ở chiều đọc một URI hỏng: ba ký tự phá được thì phá to và thấy ngay,
còn `@` thì không phá gì cả.

Chỗ hỏng im lặng nằm ở **chiều ngược lại, và nó là chiều hay dùng nhất**: `url.password` trả về
chuỗi **đã percent-encode**, khác với `pathname` và `searchParams` vốn được decode sẵn. Dán
`mysql://user:p%40ss@host/db` vào mà quên `decodeURIComponent` thì ô mật khẩu hiện ra `p%40ss`,
người dùng chép nó sang một file config, và đó là **sai mật khẩu** — không có gì báo, và triệu
chứng ở đầu kia là "sai tài khoản" chứ không phải "chuỗi hỏng".

Vậy hai luật, và bộ test giữ cả hai:

- **Đọc thì `decodeURIComponent`** cả `username` lẫn `password`. Đây là chỗ hỏng im lặng.
- **Ghi thì `encodeURIComponent`** cả hai. `/`, `?`, `#` là bắt buộc vì thiếu chúng thì chuỗi không
  parse được ở đâu cả; `@` và `:` thì không bắt buộc với `URL`, nhưng vẫn encode — chuỗi này được
  dán sang driver, sang file config và sang mắt người, và không phải chỗ nào cũng theo luật "cắt ở
  `@` cuối cùng".

Bộ test đi vòng tròn: một mật khẩu chứa cả năm ký tự, ghi ra URI rồi đọc lại, phải ra đúng mật khẩu
ban đầu.

### 4.3 JDBC chỉ cho MySQL và PostgreSQL

```
jdbc:mysql://host:3306/db?user=…&password=…
jdbc:postgresql://host:5432/db?user=…&password=…
```

**MongoDB và Redis không có chuẩn JDBC.** Có driver của bên thứ ba, nhưng chuỗi của chúng khác nhau
theo từng hãng, và in ra một chuỗi `jdbc:mongodb://…` trông hợp lệ là đưa cho người dùng một thứ
sẽ hỏng ở nơi khác. Chọn hai loại đó thì ô JDBC nói thẳng là không áp dụng, thay vì để trống — một
ô trống trông như một cái bug.

### 4.4 Dạng `.env` và docker dùng lại giai đoạn 3

`DB_HOST`, `DB_PORT`, `DB_USER`, `DB_PASSWORD`, `DB_NAME` — dựng ra `EnvPair[]` rồi đưa thẳng cho
`toEnv` và `toDockerArgs` của `tools/tools/env/env.ts`. Luật bọc ngoặc, kể cả luật shell cho dạng
docker, đã đúng ở đó rồi và không có lý do gì viết lại.

Đây là lần thứ ba hai tool trong module gọi nhau — sau `schema` gọi `case` và `diff` gọi `format` ở
giai đoạn 3 — và vẫn đúng chiều: logic thuần gọi logic thuần, không Panel nào biết Panel nào.

Tên biến cố định `DB_*` chứ không theo tên biến của image docker chính thức
(`MYSQL_ROOT_PASSWORD`, `POSTGRES_USER`…): những tên đó khác nhau theo từng image và theo từng
phiên bản, còn `DB_*` thì đoán được và sửa lại một dòng là xong.

## 5. Việc ngoài `src/modules/tools/`

- `src-tauri/src/modules/tools/` — thư mục mới: `mod.rs`, `commands.rs`, `ports.rs`.
- `src-tauri/src/modules/mod.rs` — một `pub mod tools;`, một khối lệnh mới, và sáu dòng đổi tên ở
  khối `db`.
- `src-tauri/src/modules/db/commands/tools.rs` — sáu tên hàm đổi.
- `src/modules/db/tools.ts` — sáu chuỗi `invoke` đổi.
- `src/i18n/dicts.ts` — nối `toolsEn.error` và `toolsVi.error` vào dòng gộp nhóm `error`.
- `CHANGELOG.md` — một dòng dưới `## [Unreleased]`.

Không dependency mới, cả `package.json` lẫn `Cargo.toml`.

## 6. Thứ tự làm

Phần rename đi trước mọi thứ: nó là điều kiện để lệnh mới có tên đúng, và trộn nó vào một task khác
là làm cho một commit rename thuần trở thành một commit khó đọc.

| # | Nội dung | Test |
| --- | --- | --- |
| 1 | Rename `tools_*` thành `dumptools_*` | Không có test mới; `cargo test` và `npm run build` là lưới |
| 2 | `ports.rs` — ba bộ đọc output | Fixture thật của `netstat`, `ss`, `lsof` |
| 3 | `commands.rs` + `mod.rs` + mã lỗi + nối `error` vào `dicts.ts` | — |
| 4 | Lệnh kill theo OS | Bảng ở 2.5, cả hai dạng, cả ba OS |
| 5 | Panel `ports` + registry + i18n | — |
| 6 | `paramsOf` / `fill` | Tham số lặp, tham số chưa điền, template không có tham số |
| 7 | Bộ sẵn có + thao tác thuần trên danh sách snippet | Thêm, sửa, xoá; id không đụng nhau |
| 8 | Store + Panel `cheatsheet` + registry + i18n | — |
| 9 | `parseConnectionString` và bốn bộ ghi | Bảng ở 4.1, mật khẩu ở 4.2, `+srv` không cổng |
| 10 | Panel `connection` + registry + i18n + CHANGELOG | — |
