+++
title = "Khi có gì đó không ổn"
slug = "troubleshooting"
order = 14
summary = "Chạy mix doctor trước, rồi bốn lệnh trả lời đúng những câu hỏi người dùng hay gặp, và một file gom đủ mọi thứ một báo cáo lỗi cần."
translation_of = "en/troubleshooting.md"
source_sha256 = "466aae33ce48f3f8a911d58b36cc10cd1a9596caa88c434b9aa6a7ee617b2d72"
+++

# Khi có gì đó không ổn

> **Đây là tài liệu hướng dẫn dùng MixLab qua dòng lệnh `mix`.** Nếu bạn muốn thao tác bằng
> giao diện đồ hoạ cho dễ hơn thì bạn đã có sẵn: mọi bộ cài đều đặt sẵn cửa sổ MixLab ngay cạnh
> dòng lệnh. Cả hai đều điều khiển cùng một MixEngine bên dưới, nên mọi khái niệm trong cẩm nang
> này vẫn áp dụng.

## Bắt đầu từ đây

```bash
mix doctor
```

Lệnh này kiểm tra máy và cho biết có gì sai. Nó **chỉ báo cáo, không sửa gì** trừ khi bạn yêu cầu,
và thoát với mã khác không khi phát hiện vấn đề, nên script cũng dùng được.

```bash
mix doctor --repair
```

Sửa mọi thứ có thể sửa. Những gì nằm trong home của MixEngine được sửa ngay. Những gì cần quyền
quản trị được xếp vào hàng đợi, hiện ra cho bạn xem, rồi cấp quyền trong **một** hộp thoại cho cả
đợt. `--yes` bỏ qua bước xác nhận trước hộp thoại đó.

## Bốn câu hỏi người dùng hay gặp

### "Có gì đang chạy không?"

```bash
mix status
mix service list
```

`status` nói về daemon: phiên bản, thư mục home, và nó đang giám sát gì. `service list` liệt kê
từng service và trạng thái của mỗi cái.

### "Sao tên miền này không mở được?"

```bash
mix domain status blog.test
```

Bốn câu hỏi được trả lời riêng rẽ thay vì một kết luận chung: tên đã được khai báo chưa, nó được
định tuyến bằng cách nào, hiện tại nó có phân giải được trên máy này không, và có gì đang trả lời
ở đó không. Câu nào trả lời `no` thì đó là chỗ cần sửa.

### "Sao ổ khoá không xanh?"

```bash
mix cert status
mix cert ca-status
```

`cert status` mở một kết nối thật và báo lại chứng chỉ thực sự được đưa ra, tức là thứ duy nhất
trình duyệt nhìn thấy. `ca-status` cho biết CA là gì. Nếu CA chưa được tin cậy,
`mix doctor --repair` sẽ đưa nó trở lại.

### "Đây là PHP nào, và vì sao?"

```bash
mix runtime resolve php
```

Phiên bản mà thư mục này dùng, **và nguồn nào trong bốn nguồn quyết định điều đó**. Vế sau là thứ
bạn cần khi câu trả lời không như mong đợi.

## Đọc log

```bash
mix service logs caddy --follow
mix service logs mariadb@main -n 200
```

`--follow` vẫn tiếp tục khi service crash và được khởi động lại, vì thứ đang được theo dõi là
service chứ không phải một lần chạy của tiến trình. Log của chính daemon nằm ở `logs/daemon.log`
trong home của MixEngine.

Với các thao tác dài, như cài đặt hay áp dụng blueprint, hãy xem ở job:

```bash
mix job list
mix job status <id>
mix job logs <id>
```

`mix job logs` chỉ trả lời cho job chạy chương trình của người khác, hiện tại nghĩa là blueprint
chạy lệnh scaffold của nó. Mọi việc khác một job làm đều được báo qua tiến độ và kết quả, và lệnh
nói rõ như vậy thay vì giả vờ output bị mất.

## Các tình huống thường gặp

**Cổng đã bị chiếm.** Có thứ gì khác trên máy đang dùng nó. Với service mới, dùng
`mix service create --port` để chọn cổng khác. Với service đã có, xoá rồi tạo lại trên cổng khác;
thư mục dữ liệu được giữ nguyên.

**Daemon không khởi động.** Đọc `logs/daemon.log` trong home. `mix status --no-autostart` hỏi xem
có daemon đang chạy không mà không khởi động cái mới. Đó là câu hỏi đúng khi bạn đang chẩn đoán chứ
không phải đang làm việc.

**Lệnh cần một phiên bản chưa cài.** MixLab nói rõ và nêu đúng lệnh `mix runtime install` cần
gõ. Nếu bạn yêu cầu một *khoảng* phiên bản thì nó không biết phiên bản nào thoả mãn, nên chỉ bạn
sang `mix runtime available`.

**Có gì đó xin quyền quản trị và bạn đã từ chối.** Không có gì bị áp dụng nửa chừng.
`mix elevation status` cho biết còn gì đang chờ, và `mix elevation grant` hỏi lại.

## MixLab chiếm quá nhiều dung lượng đĩa

`mix disk` chia home này thành năm nhóm và nói rõ, với từng nhóm, thứ gì lấy lại được dung lượng đó:

```
          size      reclaimed by
runtimes  700 MiB   runtime.uninstall — one runtime at a time, and never one a running pool is using
data      1200 MiB  these are your databases, and nothing in MixLab deletes them
logs      40 MiB    `mix cleanup` — 30 MiB in 4 file(s)
certs     < 1 MiB   every site would lose HTTPS until `cert.issue` ran again …
cache     90 MiB    `mix cleanup` — 90 MiB in 12 file(s)
other     310 MiB   packages, generated config, the database
```

`mix cleanup` chỉ lấy lại hai nhóm cuối và không gì khác. Nó xoá các bản log đã xoay vòng
(`daemon.log.1`, `current.log.2` của một service) và dọn sạch cache tải về. Nó không đụng tới các
file log đang được ghi ngay lúc này, các báo cáo sự cố của home, cơ sở dữ liệu của bạn, các runtime
đã cài hay các chứng chỉ: nó khớp theo *tên file* chứ không quét cả home, nên không có tham số nào
bạn truyền vào mà chạm tới được chúng.

`--keep-logs` và `--keep-cache` giữ lại một trong hai nhóm. `--yes` trả lời câu xác nhận trước, đó
là cách một script nói đồng ý.

Lệnh này từ chối chạy khi còn job khác đang chạy, vì dọn cache sẽ xoá mất file mà một lượt tải đang
tiếp tục dở. Hãy đợi job đó xong, hoặc huỷ nó bằng `mix job cancel <id>`.

Để giải phóng nhiều hơn: `mix runtime list` và `mix runtime uninstall <runtime> <version>` là thứ
lấy lại `runtimes/`, còn `mix package list` và `mix package uninstall <package> <version>` lấy lại
phần lớn của *other*.

## Khi chính MixLab gặp bug

Nếu daemon gặp bug trong mã của chính nó, nó ghi một file nhỏ vào `logs/crashes/` trong home của
MixEngine. `mix doctor` cho bạn biết có file như vậy, dưới dạng ghi chú chứ không phải vấn đề, nên
không làm đổi mã thoát của lệnh.

**Trong file có gì**: bug xảy ra ở đâu trong mã nguồn của MixLab, tên các hàm xung quanh, phiên
bản đang chạy và hệ điều hành nào. Chỉ có vậy.

**Trong file không có gì**: không có đường dẫn nào của bạn, không có tên site hay project, và không
có mật khẩu. Điều này đúng vì file *chỉ được phép chứa* những gì kể trên, chứ không phải vì có gì
đó được lọc bỏ về sau. Nên bạn có thể đính kèm nguyên file vào một báo cáo lỗi công khai mà không
cần đọc trước.

Thông báo mà crash in ra là phần duy nhất có thể nhắc tới đường dẫn của bạn, nên nó được ghi vào
`logs/daemon.log` thay vì vào file kia. File đó cũng đáng gửi kèm, nhưng nên đọc qua trước. Xem
bên dưới.

**Không có gì được gửi đi đâu cả.** Không có server nào để gửi tới. Hai mươi file mới nhất được
giữ lại, các file cũ hơn bị xoá. Nếu bạn không muốn file như vậy được ghi ra, thêm vào
`config.toml`:

```toml
[crash]
enabled = false
```

Log của daemon vẫn ghi lại rằng đã có crash.

## Báo cáo lỗi

```bash
mix doctor --bundle
```

Một file nén gom đủ mọi thứ một báo cáo lỗi cần: kết quả `doctor` tìm được, trạng thái daemon này,
thông tin máy, các báo cáo crash nếu có, và phần cuối của log. `--out` chép nó tới nơi bạn chọn.

**Những gì cố ý bỏ ra ngoài được ghi tên ngay trong file nén**, nên không ai phải đoán một phần
thiếu là do che đi hay do lỗi. Đó là một file nén bình thường, nên mở ra xem trước khi
gửi đi đâu.

Mọi lệnh `mix` đều nhận `--json`, thường là cách nhanh nhất để cho người khác thấy chính xác bạn
đã thấy gì.
