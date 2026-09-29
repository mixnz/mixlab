+++
title = "Phiên bản PHP, Node, Python, Ruby, Go và Java"
slug = "runtimes"
order = 5
summary = "Cài bao nhiêu phiên bản tuỳ bạn, và để mỗi thư mục tự chọn phiên bản của nó. Không hook shell, không phải nhớ gì cả."
translation_of = "en/runtimes.md"
source_sha256 = "27594d82cb04c2c8ace1ba4cbdded841d4ad19c4444dd881c7f77fc77e23805d"
+++

# Phiên bản PHP, Node, Python, Ruby, Go và Java

> **Đây là tài liệu hướng dẫn dùng MixLab qua dòng lệnh `mix`.** Nếu bạn muốn thao tác bằng
> giao diện đồ hoạ cho dễ hơn thì bạn đã có sẵn: mọi bộ cài đều đặt sẵn cửa sổ MixLab ngay cạnh
> dòng lệnh. Cả hai đều điều khiển cùng một MixEngine bên dưới, nên mọi khái niệm trong cẩm nang
> này vẫn áp dụng.

MixLab cài runtime ngôn ngữ vào thư mục riêng của nó, mỗi phiên bản một thư mục bất biến, và
không bao giờ đụng tới những gì hệ điều hành đã có sẵn. Cài một phiên bản mới không bao giờ sửa
phiên bản đã cài, nên bạn thêm gì vào cũng không làm hỏng thứ đang chạy tốt.

Có sáu ngôn ngữ được quản lý: **PHP**, **Node.js**, **Python**, **Ruby**, **Go** và **Java**, cùng
một công cụ là **Composer**, được cài theo cùng cách và chạy dưới PHP mà thư mục hiện tại dùng.

## Cài một phiên bản

```bash
mix runtime available --kind php
mix runtime install php 8.3.33
mix runtime list
```

Phiên bản phải ghi chính xác, và đây là cố ý. Ghi `8.3` nghĩa là *"chọn giúp
tôi một cái"*, mà chưa cài gì thì không có gì để chọn. Việc chọn giữa các phiên bản là việc của bước
resolve, và resolve chỉ trả lời dựa trên những gì có trên máy. Muốn dùng một khoảng phiên bản thì
dùng ở `mix runtime available`.

Cài đặt là một job, và mặc định `mix` sẽ chờ nó xong. Vì thế `mix runtime install php 8.3.33 && …`
đảm bảo PHP đã có mặt trước khi lệnh sau chạy. Với `--no-wait`, lệnh trả về ngay khi daemon nhận
việc và đưa bạn một job id, để sau đó bạn chờ bằng `mix job wait`.

**Cài PHP cũng tạo luôn pool php-fpm** cho phiên bản đó, ví dụ `php-fpm@8.3.33`. Đây là một
service như mọi service khác, xuất hiện trong `mix service list`. Node, Python, Ruby, Go và Java
được gọi theo từng lệnh, không có gì cần giám sát.

### Trên máy Windows dùng chip ARM

Một số phiên bản không có bản build cho chip này, ví dụ không ai phát hành PHP cho Windows ARM64.
Trong trường hợp đó, MixLab cài bản x86_64 và Windows sẽ chạy nó cho bạn. Vẫn chạy được, chỉ
chậm hơn một chút so với bản build đúng cho máy.

Bạn không phải đoán cái nào là cái nào. Trên máy đó, `mix runtime available` và
`mix package available` có thêm cột `RUNS`, ghi `native` hoặc `emulated` cho từng phiên bản, và
lệnh cài sẽ nói rõ trước khi bắt đầu tải. Trên các máy khác không có cột này, vì không có gì để
nói.

## Composer

```bash
mix runtime available --kind composer   # the versions the index offers
mix runtime install composer 2.10.3     # exact, like every install
composer --version                      # runs composer.phar under this directory's PHP
mix project update shop --pin composer=2.2
```

Composer là một file chứ không phải một chương trình: lệnh `composer` khởi động PHP mà thư mục của
bạn resolve ra, rồi đưa `composer.phar` cho PHP đó. Vì vậy `MIXENGINE_PHP=8.1 composer install`
dùng PHP 8.1, và một thư mục pin PHP 7.4 cần dòng 2.2, vì Composer 2.3 trở lên đòi PHP 7.2.5 hoặc
mới hơn.

| PHP của bạn | Pin |
| --- | --- |
| 7.2.5 trở lên | `composer = "2"` |
| 5.3 – 7.2.4 | `composer = "2.2"` |

Cài Composer không tạo service nào và không chạy gì cả; `mix runtime list` hiện nó cạnh các ngôn
ngữ.

## Go

```bash
mix runtime available --kind go        # 1.21 to the newest release
mix runtime install go 1.25.14
go version                             # the Go this directory resolves to
mix project update api --pin go=1.25
```

`go` và `gofmt` là lệnh như mọi lệnh khác. `GOROOT` do chính `go` tự suy ra từ nơi nó được cài, còn
`GOPATH`, module cache và build cache vẫn nằm ở chỗ Go đặt. Mọi phiên bản dùng chung chúng, và Go
vốn được thiết kế để dùng như vậy.

**Go đã pin là Go dùng để build.** Một `go.mod` đòi phiên bản mới hơn phiên bản thư mục của bạn
resolve ra sẽ không âm thầm tải phiên bản đó về rồi chạy thay: `go` được khởi động qua MixLab
chạy với `GOTOOLCHAIN=local`, nên module như vậy dừng lại với thông báo của chính Go.

```text
go: go.mod requires go >= 1.27 (running go 1.25.14; GOTOOLCHAIN=local)
```

Cách xử lý là cài phiên bản Go mới hơn rồi pin nó. Ba chi tiết:

- `GOTOOLCHAIN` do bạn tự export được giữ nguyên đúng như bạn viết.
- Giá trị rỗng được coi như chưa đặt, giống cách Go đọc nó.
- Giá trị bạn lưu bằng `go env -w` sẽ bị ghi đè: pin thắng một thiết lập áp dụng cho cả máy.

`mix doctor` sẽ báo khi chính MixEngine được khởi động với một `GOTOOLCHAIN` khác `local`, hoặc với
`GOROOT`, vì các lệnh nó khởi động thừa hưởng những biến đó.

Chương trình bạn thêm bằng `go install` nằm trong `GOBIN` của Go (mặc định là `~/go/bin` nếu bạn
chưa đổi), và MixLab không đưa thư mục đó vào `PATH` của bạn.

## Java

```bash
mix runtime available --kind java      # các dòng hỗ trợ dài hạn: 11, 17, 21 và 25
mix runtime install java 21
java --version                         # JDK mà thư mục này resolve ra
mix project update api --pin java=21
```

`java`, `javac`, `jar`, `jshell`, `keytool` và `jlink` là lệnh như mọi lệnh khác. Mỗi lệnh được khởi
động với **`JAVA_HOME` trỏ đúng JDK của nó**, kể cả khi bạn đã export một giá trị khác, nên chương
trình, và cả tiến trình con mà nó tự khởi động, đều tìm thấy đúng JDK mà thư mục này pin.

**Maven và Gradle gõ thẳng trong terminal đọc `JAVA_HOME` của bạn trước.** Nếu biến đó trỏ vào một
JDK hệ thống thì `mvn` và `./gradlew` dùng JDK đó, bất kể thư mục pin gì; bỏ biến đó đi thì chúng sẽ
tìm thấy `java` đã pin trên `PATH`. `mix doctor` sẽ báo khi chính MixEngine được khởi động với một
`JAVA_HOME` nằm ngoài các JDK của nó.

**HTTPS tới site của bạn chạy được.** MixLab ghi chứng chỉ gốc của mình vào kho chứng chỉ của
từng JDK đã cài, nên `https://blog.test` được Java xác thực mà không cần cờ hay tham số nào thêm.
Có hai giới hạn đáng biết: runtime bạn tự dựng bằng `jlink` mang kho chứng chỉ gốc ban đầu nên không
tin các site này, và một JVM khởi động kèm `-Djavax.net.ssl.trustStore` sẽ đọc kho đó thay vì kho
của JDK. Nếu một JDK mất chứng chỉ này, `mix doctor --repair` ghi lại.

**Trên Linux, JDK cần một số thư viện của hệ thống**: `zlib` để chạy được, `freetype` để dựng chữ,
X11 cho cửa sổ và ALSA cho âm thanh. Khi máy bạn thiếu thư viện nào, bản cài sẽ nói tên thư viện đó
rồi vẫn cài tiếp: một server không vẽ cửa sổ và không phát âm thanh vẫn chạy tốt mà không cần chúng,
và trình quản lý gói của bản phân phối có sẵn chúng khi bạn cần.

## Chọn phiên bản cho từng thư mục

Không có gì ở đây sửa shell, vá file profile, hay bắt bạn gõ lệnh activate. Mỗi thư mục resolve ra
một phiên bản, phần còn lại do shim lo.

```bash
mix runtime default php 8.3.33      # the machine-wide fallback
mix project update blog --pin php=^8.1
mix runtime resolve php             # what does *this* directory get, and why?
```

`mix runtime resolve` là lệnh đáng nhớ nhất. Nó trả lời đúng thứ `php -v` sẽ trả lời mà không chạy
gì cả, **và** nói rõ nguồn nào trong bốn nguồn sau quyết định điều đó:

1. Cờ hoặc biến môi trường truyền tường minh cho lệnh đang chạy.
2. File `mixengine.toml` gần nhất có nhắc tới ngôn ngữ này, tìm ngược lên từ thư mục hiện tại.
3. Project đã đăng ký bao trùm thư mục này.
4. Giá trị mặc định toàn cục.

Một file `mixengine.toml` không nói gì về PHP thì không phải câu trả lời cho PHP, nên pin ở lớp
ngoài vẫn có hiệu lực.

### Cách viết ràng buộc phiên bản

Pin và `--version` chấp nhận ba dạng. Tất cả đều resolve trên các phiên bản **đã cài**, không bao
giờ âm thầm lấy từ danh sách có thể tải:

| Cách viết | Nghĩa |
| --- | --- |
| `8.3.33` | Đúng phiên bản đó |
| `8.3` hoặc `8` | Các phần được ghi phải khớp; phần không ghi coi như số không |
| `^8.3` | Khớp tới phần khác không ở ngoài cùng bên trái; `^0.12` dừng trước `0.13` |

Ràng buộc không ghi pre-release thì không bao giờ chọn pre-release. `8.5` và `^8.5` đều bỏ qua
`8.5.0RC1`; muốn dùng nó thì phải ghi chính xác tên.

## Shim

`mix path install` đưa `<root>/bin` vào `PATH` của bạn. Trong đó có một chương trình nhỏ cho mỗi
lệnh của những ngôn ngữ bạn đã cài bằng MixEngine: `php`, `composer`, `node`, `npm`, `python`,
`pip`, `ruby`, `go`, `java` và các lệnh khác. Mỗi chương trình tự tìm xem thư mục hiện tại muốn
phiên bản nào rồi chuyển cho file thực thi thật.

Ba hệ quả đáng biết:

- **Hoạt động cả khi daemon đã dừng.** Shim đọc trực tiếp thứ nó cần thay vì hỏi qua socket. Vì
  vậy `php -v` trong một project vẫn trả lời được khi MixEngine không chạy.
- **Chỉ có những gì bạn đã cài.** Cài bản Node.js đầu tiên thì có `node`, `npm`, `npx`; gỡ bản
  cuối cùng thì chúng mất. Ngôn ngữ bạn chưa từng cài bằng MixEngine không có lệnh nào trong
  `<root>/bin`, nên `which node` sẽ tìm thấy bản Node.js bạn tự cài.
- **Terminal mở từ trước có thể còn nhớ đường dẫn cũ.** Sau khi một lệnh xuất hiện hoặc mất đi, mở
  terminal mới, hoặc gõ `hash -r` trong bash.

Chỉ có `<root>/bin` được đưa vào `PATH`. Một mục duy nhất, không bao giờ là một thư mục cho mỗi
phiên bản.

```bash
mix path status
mix path uninstall
```

`mix path uninstall` gỡ thư mục đó khỏi `PATH` nhưng để nguyên các lệnh bên trong. Chúng nằm trong
thư mục home của MixEngine, và chỉ khi gỡ home thì chúng mới mất.

## Extension của PHP

Extension gắn với từng phiên bản đã cài, vì chúng được biên dịch ứng với phiên bản đó:

```bash
mix runtime ext list --php 8.3.33
mix runtime ext enable redis --php 8.3.33
mix runtime ext disable xdebug --php 8.3.33
```

`list` cho biết bản build có những extension nào, **và vì sao mỗi cái đang bật hay tắt**. Đó
thường mới là câu hỏi thật. Bỏ `--php` thì lấy phiên bản mà thư mục hiện tại resolve ra.

Bật một extension nghĩa là mọi tiến trình PHP của phiên bản đó đều nạp nó, kể cả pool.

## Gỡ một phiên bản

```bash
mix runtime uninstall php 8.1.31
```

Lệnh này bị từ chối khi còn project đã đăng ký đang pin phiên bản đó, và MixLab sẽ nêu tên các
project ấy. Nó cũng bị từ chối khi pool php-fpm chạy từ phiên bản đó vẫn đang chạy. `--force` bỏ
qua được điều kiện thứ nhất, nhưng không bao giờ bỏ qua điều kiện thứ hai.
