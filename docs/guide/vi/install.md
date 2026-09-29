+++
title = "Cài đặt MixLab"
slug = "install"
order = 2
summary = "Bộ cài cho hệ điều hành của bạn, nó đụng vào những gì, cố ý không đụng vào những gì, và cách kiểm tra file vừa tải."
translation_of = "en/install.md"
source_sha256 = "91ae4ff47fa21e964a1ae2ba6712d8a3998d37c784865252b813760b421e6def"
+++

# Cài đặt MixLab

> **Đây là tài liệu hướng dẫn dùng MixLab qua dòng lệnh `mix`.** Nếu bạn muốn thao tác bằng
> giao diện đồ hoạ cho dễ hơn thì bạn đã có sẵn: mọi bộ cài đều đặt sẵn cửa sổ MixLab ngay cạnh
> dòng lệnh. Cả hai đều điều khiển cùng một MixEngine bên dưới, nên mọi khái niệm trong cẩm nang
> này vẫn áp dụng.

Mọi bản build đều được phát hành trên trang releases của dự án trên GitHub, kèm theo checksum và
chữ ký. Bạn chọn file đúng với hệ điều hành của mình ở bên dưới. Bộ cài thay đổi máy bạn ít nhất
có thể: chưa có gì được thêm vào kho chứng chỉ, cài đặt DNS hay firewall cho tới khi bạn yêu cầu một
tính năng cần tới chúng. Chi tiết xem ở [MixLab xin quyền để làm gì](./permissions.md).

**Hiện chưa có bản phát hành ổn định.** Mọi link tải bên dưới là URL cố định, GitHub luôn trỏ nó
tới bản mới nhất *không phải* pre-release. Vì vậy khi bản ổn định đầu tiên ra mắt, các link này sẽ
tự hoạt động mà không cần sửa trang này. Trong lúc chờ, bạn lấy bản pre-release mới nhất thủ công
tại [trang releases](https://github.com/mixnz/mixlab/releases). Hiện tại đó là `v0.0.13`.

## Bạn đang cài những gì

Có năm chương trình. Nên biết mỗi cái làm gì trước khi một trong số chúng làm bạn bất ngờ.

| Chương trình | Nhiệm vụ |
| --- | --- |
| `mixengined` | Daemon. Lưu mọi trạng thái MixEngine biết và giám sát mọi tiến trình MixEngine chạy. |
| `mix` | Lệnh bạn gõ. Nó hỏi daemon rồi in câu trả lời ra. |
| `mixengine-shim` | Chương trình thế chỗ cho `php`, `node`, `python` và `ruby`, chọn đúng phiên bản cần chạy. |
| `mixengine-elevate` | Chương trình duy nhất chạy với quyền quản trị, mỗi lần chỉ vài giây. |
| **MixLab** | Cửa sổ: bảng điều khiển cho daemon, kèm một client cơ sở dữ liệu, một client HTTP và một terminal. |

Ba chương trình đầu và MixLab được cài chung một lượt. Trên Windows chúng được cài dưới tài khoản
của bạn, và bộ cài không đặt `mixengine-elevate` vào máy: MixLab tự cài nó vào lần đầu tiên có việc
cần quyền quản trị, ngay trong hộp thoại xin quyền mà đằng nào bạn cũng sẽ thấy. `.pkg`, `.deb` và
`.rpm` thì đặt nó vào máy với quyền root ngay lúc cài. Dù cài cách nào, MixLab cũng tự cập nhật nó:
khi một bản cập nhật thay đổi nó, lần xin quyền kế tiếp sẽ thay nó.

**Nếu bạn không cần cửa sổ, có bản tải không kèm nó.** Mỗi hệ điều hành đều phát hành một bản
**headless** chỉ chứa bốn chương trình dòng lệnh và không gì khác: không có cửa sổ, và trên Linux
cũng không cần cài WebKitGTK. Link nằm trong từng mục bên dưới, và đó là bản dành cho máy chủ, image
container, hay bất kỳ máy nào không có màn hình.

## Windows

[**Tải bộ cài**](https://github.com/mixnz/mixlab/releases/latest/download/mixlab-windows-x86_64-setup.exe)
· [bộ cài headless](https://github.com/mixnz/mixlab/releases/latest/download/mixengine-windows-x86_64-headless-setup.exe)
· Windows ARM: [bộ cài](https://github.com/mixnz/mixlab/releases/latest/download/mixlab-windows-aarch64-setup.exe),
[headless](https://github.com/mixnz/mixlab/releases/latest/download/mixengine-windows-aarch64-headless-setup.exe)

Có hai bộ cài được phát hành, bộ nào cũng là một bản cài đầy đủ.

- **`mixlab-<version>-windows-x86_64-setup.exe`**: bộ cài theo từng người dùng. Nó ghi vào
  profile của bạn và thêm thư mục cài vào `PATH`, nên lúc cài không cần quyền quản trị, và cũng
  không đụng tới tài khoản của người khác trên cùng máy. Nó còn thêm **MixLab** vào Start Menu, cho
  bạn chọn tạo shortcut ngoài desktop ở trang thành phần, và đặt MixLab làm chương trình mở link
  `mixlab://`.
- **`mixengine-<version>-windows-x86_64-headless-setup.exe`**: cũng bộ cài đó nhưng không có
  MixLab, chỉ bốn chương trình dòng lệnh và không gì khác.

Lần đầu MixLab cần quyền quản trị, thường là lúc bạn cho phép bước thiết lập đầu tiên, cùng một hộp
thoại đó cũng đặt chương trình phụ trợ đặc quyền vào chỗ.

Bản cho Windows ARM được phát hành bên cạnh, đặt tên `aarch64`.

**Bạn sẽ gặp cảnh báo SmartScreen.** MixLab chưa được ký bằng chứng chỉ Authenticode, nên
Windows hiện *"Windows protected your PC"* và giấu nút chạy sau **More info → Run anyway**. Cảnh
báo này chỉ nói rằng chưa ai mua chứng chỉ, chứ không nói gì về bản thân file. Nếu muốn biết chắc
file mình tải có đúng không, hãy kiểm tra chữ ký theo hướng dẫn bên dưới. Cảnh báo này thường xuất
hiện lại ở mỗi bản phát hành, vì khi không có danh tính nhà phát hành, "uy tín" mà Windows tích luỹ
gắn với từng file chứ không gắn với dự án.

## macOS

[**Tải gói cài**](https://github.com/mixnz/mixlab/releases/latest/download/mixlab-macos-universal.pkg)
· [gói headless](https://github.com/mixnz/mixlab/releases/latest/download/mixengine-macos-universal-headless.pkg)

**`mixlab-<version>-macos-universal.pkg`**: một gói dùng chung cho cả Intel lẫn Apple silicon. Nó
đặt các chương trình dòng lệnh vào `/usr/local/bin` và **MixLab** vào `/Applications`, nên cửa sổ có
mặt trong Spotlight và Launchpad ngay khi cài xong.

**`mixengine-<version>-macos-universal-headless.pkg`** cài đúng bốn chương trình dòng lệnh đó và
cùng chương trình phụ trợ đặc quyền, nhưng không kèm MixLab, dành cho máy không cần cửa sổ. Khi
cập nhật, mỗi máy Mac nhận đúng loại gói nó đang dùng.

MixLab cũng chưa có Apple Developer ID, nên nếu bạn nhấp đúp gói cài trong Finder thì sẽ gặp hộp
thoại Gatekeeper. Trên macOS 15 trở lên còn phải vào **System Settings → Privacy & Security → Open
Anyway**. Cài từ terminal thì bỏ qua được tất cả những bước đó:

```bash
sudo installer -pkg mixlab-*-macos-universal.pkg -target /
```

Với một sản phẩm dòng lệnh thì đây là cách nên dùng trước tiên. Gói cài chạy với quyền root, nên nó
cũng đặt luôn chương trình phụ trợ cần quyền quản trị vào máy cho bạn.

## Linux

[**`.deb`**](https://github.com/mixnz/mixlab/releases/latest/download/mixlab_amd64.deb)
· [**`.rpm`**](https://github.com/mixnz/mixlab/releases/latest/download/mixlab-x86_64.rpm)
· arm64: [`.deb`](https://github.com/mixnz/mixlab/releases/latest/download/mixlab_arm64.deb),
[`.rpm`](https://github.com/mixnz/mixlab/releases/latest/download/mixlab-aarch64.rpm)
· headless: [`.deb`](https://github.com/mixnz/mixlab/releases/latest/download/mixengine-headless_amd64.deb),
[`.rpm`](https://github.com/mixnz/mixlab/releases/latest/download/mixengine-headless-x86_64.rpm),
arm64 [`.deb`](https://github.com/mixnz/mixlab/releases/latest/download/mixengine-headless_arm64.deb),
[`.rpm`](https://github.com/mixnz/mixlab/releases/latest/download/mixengine-headless-aarch64.rpm)

Mỗi họ bản phân phối có hai gói, gói nào cũng là một bản cài đầy đủ:

- **`mixlab`**: MixLab và bốn chương trình dòng lệnh
- **`mixengine-headless`**: chỉ bốn chương trình dòng lệnh, cho máy chủ hoặc máy không có màn hình

`.deb` dành cho Debian, Ubuntu và các bản phái sinh, `.rpm` dành cho Fedora, RHEL và openSUSE:

```bash
sudo apt install ./mixlab_*_amd64.deb
sudo dnf install ./mixlab-*.x86_64.rpm
```

Hai gói thay thế nhau: cài gói này thì gói kia bị gỡ.

**Các bản phân phối khác không được hỗ trợ.** Arch, NixOS, Gentoo và những bản còn lại không có gói
ở đây; muốn dùng thì build từ mã nguồn (mục bên dưới).

**Cửa sổ cần WebKitGTK 4.1 và glibc 2.35**: Ubuntu 22.04, Debian 12, Fedora 38, openSUSE Leap 15.6
trở lên. MixLab là ứng dụng webview, và thư viện nó vẽ lên là thư viện của bản phân phối bạn đang
dùng: `libwebkit2gtk-4.1-0` trên Debian và Ubuntu, `webkit2gtk4.1` trên Fedora và RHEL,
`libwebkit2gtk-4_1-0` trên openSUSE. Gói `mixlab` khai báo phụ thuộc này nên trình quản lý gói tự
kéo về; gói đó cũng thêm một mục menu **MixLab** kèm icon.

**Gói headless không cần gì trong số đó.** Bốn chương trình dòng lệnh được build với glibc 2.28, nên
chạy được trên các bản phân phối hỗ trợ dài hạn mà chúng nhắm tới, và không cần webview.

**Bản cập nhật là gói tiếp theo.** `mix self-update` tải gói về, kiểm tra với bản phát hành đã ký,
rồi in lệnh cài nó: `sudo apt install …` hoặc `sudo dnf install …`. Trên máy có desktop, nó mở luôn
gói đó trong trình quản lý phần mềm.

Bản `aarch64` được phát hành bên cạnh bản `x86_64`.

## Build từ mã nguồn

MixLab viết bằng Rust, và chỉ Rust:

```bash
git clone https://github.com/mixnz/mixlab.git
cd mixlab
cargo build --release
```

Các file thực thi nằm trong `target/release/`. MixLab được build riêng, vì nó là một workspace độc
lập nằm dưới `apps/desktop/`. Build từ mã nguồn là thêm một cách cài chạy hoàn toàn dưới tài khoản
của bạn. Cũng vì thế mà việc đặt chương trình phụ trợ cần quyền quản trị không bao giờ là việc của
người đóng gói.

## Kiểm tra file bạn vừa tải

Bên cạnh mỗi file phát hành có hai file đi kèm, và chúng trả lời hai câu hỏi khác nhau.

```bash
sha256sum -c mixlab_*_amd64.deb.sha256
minisign -Vm mixlab_*_amd64.deb -P <the key in packaging/updates.pub>
```

File `.sha256` cho bạn biết hai lần tải cùng một file có ra đúng cùng một file hay không. **Nó
không phải chữ ký** và cũng không được coi là chữ ký: ai thay được file phát hành thì cũng thay
được checksum nằm cạnh nó. Các file không gắn số phiên bản mà những link ở trên trỏ tới cũng có
`.sha256` và `.minisig` riêng, đặt tên theo chính chúng chứ không theo file có số phiên bản mà chúng
là bản sao. File `.minisig` mới là câu trả lời thật: đó là chữ ký Ed25519 do chính pipeline phát
hành của MixLab tạo ra, ứng với khoá công khai được commit trong repo của dự án tại
`packaging/updates.pub` và được biên dịch vào MixEngine. Đây cũng chính là khoá mà `mix self-update`
kiểm tra trước khi thay bất cứ thứ gì.

## Sau khi cài

Mở một cửa sổ terminal mới, vì bộ cài đã sửa `PATH` mà shell đang chạy sẵn thì chưa biết chuyện đó.
Rồi gõ:

```bash
mix status
```

Lệnh `mix` đầu tiên sẽ khởi động daemon nếu nó chưa chạy. Kết quả bạn nên thấy là một daemon khoẻ
mạnh, số phiên bản của nó, và chưa có gì đang được giám sát.

Tiếp theo, đưa các lệnh runtime vào `PATH`. Đây là một bước riêng vì chúng nằm ở một thư mục riêng:

```bash
mix path install
```

Lệnh này điền vào `<root>/bin` các shim, để `php`, `node`, `python` và `ruby` trỏ tới đúng phiên
bản mà từng thư mục yêu cầu, thay vì một phiên bản chung cho cả máy. MixLab cũng làm được việc
này ngay trên Dashboard, hoặc bằng công tắc **Lệnh trong terminal** trong Settings.

Nếu bạn có cài cửa sổ, hãy mở **MixLab** từ Start Menu, `/Applications`, menu ứng dụng của
desktop, hoặc chạy `mixlab`. Nó hiển thị đúng daemon mà `mix status` vừa trả lời.

## Những gì bộ cài không làm

Không đụng gì ngoài tài khoản của bạn, và không đụng gì tới phần còn lại của máy:

- **Không cài certificate authority** nào. Việc đó diễn ra lần đầu bạn yêu cầu HTTPS.
- **Không sửa DNS hay file hosts.** Việc đó diễn ra lần đầu bạn tạo site.
- **Không thêm rule firewall** và **không cấp quyền dùng cổng**. Việc đó diễn ra khi một site cần.
- **Không tải runtime hay server nào.** MixLab cài PHP, MariaDB và các thứ khác khi bạn yêu cầu,
  và chỉ những phiên bản bạn yêu cầu.
- **Không đăng ký chạy khi đăng nhập.** Chỉ khi bạn chạy `mix autostart enable` thì điều đó mới xảy
  ra.

Từng mục trên đều được mô tả ở [MixLab xin quyền để làm gì](./permissions.md), kể cả việc mỗi
hộp thoại sẽ thay đổi chính xác cái gì trước khi bạn đồng ý.

Sẵn sàng chưa? [Site đầu tiên của bạn](./getting-started.md) mất khoảng năm phút.
