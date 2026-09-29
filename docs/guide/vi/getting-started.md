+++
title = "Site đầu tiên của bạn"
slug = "getting-started"
order = 3
summary = "Từ máy vừa cài xong tới https://blog.test với ổ khoá xanh, mất khoảng năm phút."
translation_of = "en/getting-started.md"
source_sha256 = "148380e56cd9ed182eb7fbfa6e2f2e4ac54e363658ad279bce3aa31279915ceb"
+++

# Site đầu tiên của bạn

> **Đây là tài liệu hướng dẫn dùng MixLab qua dòng lệnh `mix`.** Nếu bạn muốn thao tác bằng
> giao diện đồ hoạ cho dễ hơn thì bạn đã có sẵn: mọi bộ cài đều đặt sẵn cửa sổ MixLab ngay cạnh
> dòng lệnh. Cả hai đều điều khiển cùng một MixEngine bên dưới, nên mọi khái niệm trong cẩm nang
> này vẫn áp dụng.

Trang này dẫn bạn đi trọn một vòng: cài một phiên bản PHP, một web server, tạo một project, một
site, và có chứng chỉ mà trình duyệt chấp nhận. Trang giả định bạn đã cài MixLab, nếu chưa thì
xem [Cài đặt MixLab](./install.md). Ngoài ra không giả định gì thêm.

## 1. Kiểm tra daemon

```bash
mix status
```

Lệnh `mix` đầu tiên sẽ tự khởi động daemon nếu nó chưa chạy, nên đây cũng là cách để biết bản cài
đã hoạt động. Kết quả trả về gồm phiên bản của daemon, thư mục home của nó nằm ở đâu, và nó đang
giám sát những gì. Lúc này thì chưa có gì cả.

## 2. Cài một phiên bản PHP

MixLab không kèm sẵn runtime nào. Nó chỉ tải về đúng những phiên bản bạn yêu cầu. Xem có gì rồi
chọn một cái:

```bash
mix runtime available --kind php
mix runtime install php 8.3.33
```

Phiên bản phải ghi chính xác, không phải một khoảng, và đây là cố ý. Nếu ghi `8.3` thì bạn đang bảo
MixLab chọn giữa những phiên bản mà chưa cái nào có trên máy. `mix runtime list` cho biết bạn
đang có gì.

## 3. Cài và tạo một web server

**Package** là một chương trình MixLab biết cách chạy. **Service** là một instance đang chạy của
package đó, với cấu hình riêng. Caddy là front end mặc định:

```bash
mix package available
mix package install caddy 2.11.4
mix service create caddy 2.11.4
mix service list
```

Số phiên bản thay đổi theo thời gian, nên hãy lấy từ danh sách mà `mix package available` thật sự
in ra, đừng chép từ trang này. Caddy chạy một lần cho cả home chứ không phải mỗi site một bản. Vì
vậy id service của nó không có phần `@name`: `mariadb@main` là tên một instance cụ thể, còn `caddy`
là cái duy nhất có.

## 4. Đăng ký một project

**Project** là một thư mục mà MixLab biết tới. Vào thư mục bạn muốn phục vụ, hoặc tạo một thư
mục trống nếu chỉ đang thử, rồi đăng ký:

```bash
mkdir -p ~/code/blog && cd ~/code/blog
echo '<?php phpinfo();' > index.php
mix project create
```

Không truyền tham số thì lệnh lấy thư mục hiện tại và đặt tên project theo tên thư mục. Ở đây
project sẽ tên là `blog`.

## 5. Khai báo một site

```bash
mix site create --domain blog.test --kind php-fpm --https true
```

**Đây là bước sẽ xin quyền**, và trên máy mới thì đây là bước duy nhất xin quyền. MixLab cần
tên `blog.test` trỏ về chính máy bạn, và cần trình duyệt tin chứng chỉ mà nó sắp cấp. Nó gom cả hai
việc đó, cộng thêm quyền lắng nghe trên cổng 80 và 443 nếu hệ điều hành coi đó là đặc quyền, rồi
hiện **một** hộp thoại duy nhất cho tất cả. Muốn xem chính xác nó xin gì trước khi đồng ý, chạy
`mix elevation status`. Trang [MixLab xin quyền để làm gì](./permissions.md) giải thích từng
mục.

Từ chối cũng được. Site vẫn được tạo và vẫn chạy qua `http://`.

## 6. Mở site

```bash
mix site list
```

Rồi mở `https://blog.test` trên trình duyệt. Bạn sẽ thấy trang `phpinfo()` và một ổ khoá, không có
cảnh báo nào. Nếu ổ khoá không xanh, hãy hỏi thẳng server thay vì đoán:

```bash
mix cert status
```

Lệnh này mở một kết nối TLS thật tới front end của bạn cho từng site, rồi báo lại chứng chỉ mà
server thực sự đưa ra. Đó cũng là thứ duy nhất trình duyệt nhìn thấy.

## 7. Thêm cơ sở dữ liệu, nếu project cần

```bash
mix package install mariadb 12.3.2
mix service create mariadb@main 12.3.2
mix database create mariadb@main --name blog
```

Lệnh cuối tạo cơ sở dữ liệu và một tài khoản để truy cập nó. **Mật khẩu không được in ra**: nó
được lưu vào credential store của hệ điều hành, và thứ được in ra là địa chỉ nơi nó được lưu.
`mix database open` đưa mật khẩu thẳng cho ứng dụng quản lý cơ sở dữ liệu trên máy, không bao giờ
để nó lọt vào lịch sử shell hay danh sách tham số.

## Chuyện gì vừa xảy ra

- MixLab tải một bản PHP và một web server vào thư mục riêng của nó. Không có gì được cài ở mức
  hệ thống, và không phiên bản nào khác trên máy bạn bị đụng tới.
- Nó tạo một certificate authority, xin quyền tin cậy CA đó một lần, và cấp chứng chỉ 90 ngày cho
  `blog.test`. Nó sẽ tự cấp lại chứng chỉ này trước khi hết hạn mà bạn không phải làm gì.
- Nó tự viết file cấu hình cho web server. Cấu hình đó dùng xong bỏ: MixLab sinh lại từ những gì
  nó biết, nên bạn không có file nào phải giữ cho đồng bộ.

## Đọc tiếp

- [Dự án và site](./projects-and-sites.md): hai khái niệm cốt lõi, và mỗi cái quản gì.
- [Phiên bản PHP, Node, Python, Ruby, Go và Java](./runtimes.md): cách một thư mục tự chọn phiên
  bản.
- [Máy chủ, cơ sở dữ liệu và bộ nhớ đệm](./services.md): mọi thứ một project cần để chạy.
- [Tên miền và ổ khoá](./domains-and-https.md): `blog.test` phân giải thế nào, và ai ký chứng chỉ.
- [MixLab xin quyền để làm gì](./permissions.md): từng hộp thoại, và nó thay đổi gì.
- [Khi có gì đó không ổn](./troubleshooting.md): chạy `mix doctor` trước đã.
