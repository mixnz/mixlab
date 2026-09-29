+++
title = "Dự án và site"
slug = "projects-and-sites"
order = 4
summary = "Hai khái niệm cốt lõi của MixLab, mỗi cái quản những gì, và cách một bản checkout mang theo cấu hình của chính nó."
translation_of = "en/projects-and-sites.md"
source_sha256 = "1f1833a6e87f005e4f4d884a319ca4e9d82c9019a6a58f088ac5b820f799f1a0"
+++

# Dự án và site

> **Đây là tài liệu hướng dẫn dùng MixLab qua dòng lệnh `mix`.** Nếu bạn muốn thao tác bằng
> giao diện đồ hoạ cho dễ hơn thì bạn đã có sẵn: mọi bộ cài đều đặt sẵn cửa sổ MixLab ngay cạnh
> dòng lệnh. Cả hai đều điều khiển cùng một MixEngine bên dưới, nên mọi khái niệm trong cẩm nang
> này vẫn áp dụng.

MixLab có hai khái niệm chính, và bạn nên phân biệt rõ chúng.

**Project** là một thư mục trên đĩa mà MixLab biết tới. Nó giữ đường dẫn, một cái tên, và thông
tin thư mục đó dùng phiên bản ngôn ngữ nào.

**Site** là thứ được phục vụ ra ngoài, nằm dưới một project. Nó giữ một hoặc nhiều tên miền, thư mục
nào được phục vụ, và service nào phục vụ nó. Một project không có site nào là chuyện bình thường:
đó đơn giản là một thư mục mà MixLab biết phiên bản PHP của nó. Một project có thể có nhiều site.

## Đăng ký một project

```bash
cd ~/code/blog
mix project create
mix project list
mix project show blog
```

Không truyền tham số thì `mix project create` lấy thư mục hiện tại và đặt tên project theo tên thư
mục. `--name` cho bạn đặt tên khác, còn `--pin` ghim một phiên bản ngôn ngữ cho mọi thứ nằm dưới
thư mục đó:

```bash
mix project create --name blog --pin php=^8.3 --pin node=22
```

Về sau muốn sửa gì thì dùng `mix project update`. Một điểm cần nhớ: `--pin` **thay thế** toàn bộ
danh sách pin chứ không thêm vào, và `--clear-pins` mà không kèm `--pin` sẽ xoá hết. Xoá project
chỉ là MixLab quên nó đi; file của bạn vẫn còn nguyên.

## Khai báo một site

```bash
mix site create --domain blog.test --kind php-fpm --https true
mix site list
mix site show blog.test
```

`--doc-root` là thư mục được phục vụ, tính tương đối so với gốc project. Với phần lớn framework PHP
hiện đại thì đó là `public`; bỏ trống thì lấy chính gốc project. `--domain` có thể truyền nhiều
lần: cái đầu tiên là tên miền **chính**, các cái sau là alias. Tên miền chính quan trọng vì URL
chuẩn của site và chứng chỉ đều lấy tên theo nó.

`mix site update` dùng để sửa site. Giống `--pin` ở trên, `--domain` và `--service` thay thế giá
trị cũ của site chứ không thêm vào. Không truyền cái nào thì không cái nào đổi.

Bật và tắt site chỉ đổi một cờ rồi sinh lại cấu hình:

```bash
mix site stop blog.test
mix site start blog.test
```

Hai lệnh này không khởi động hay tắt tiến trình nào. Site chỉ là một bản khai báo; các service nó
dùng có trạng thái riêng.

## Bốn loại site

| `--kind` | Là gì |
| --- | --- |
| `php-fpm` | PHP, chạy qua pool của phiên bản mà thư mục này resolve ra |
| `static` | Chỉ phục vụ file, không có gì chạy |
| `reverse-proxy` | Chuyển tiếp mọi thứ tới một địa chỉ bạn đã có sẵn đang lắng nghe, qua `--upstream` |
| `node-app` | Một tiến trình Node bạn tự chạy, trên một cổng, qua `--port` |

`reverse-proxy` và `node-app` là hai loại đáng quan tâm khi bạn đã có sẵn thứ gì đó đang chạy.
MixLab gán cho nó một tên miền thật và một chứng chỉ, mà không can thiệp vào cách bạn khởi động
nó.

## `mixengine.toml`, và nhận checkout của đồng nghiệp

Một project có thể tự mô tả chính nó trong một file commit vào repo:

```toml
[project]
name = "blog"

[runtimes]
php = "8.3"
node = "22"

[site]
domain = "blog.test"
aliases = ["api.blog.test"]
doc_root = "public"
kind = "php-fpm"
https = true

[[services]]
name = "mariadb"
version = "11.4"
database = "blog"
```

Khi có file này, chạy `mix project create` rồi `mix site create` không cần tham số nào, MixLab sẽ
làm đúng như file mô tả. Nhận checkout của người khác trông đúng như vậy: clone về, gõ hai lệnh, và
bạn có cùng phiên bản PHP, cùng tên miền với người đã viết file đó.

Theo chiều ngược lại, `mix project export` ghi project hiện tại ra `<root>/mixengine.toml`, và giữ
nguyên mọi thứ khác đã có trong file.

## Thư mục này dùng phiên bản nào?

Có bốn nguồn có thể quyết định, và chúng được xét theo thứ tự sau:

1. Cờ hoặc biến môi trường truyền tường minh cho lệnh bạn đang chạy.
2. File `mixengine.toml` gần nhất có nhắc tới **ngôn ngữ này**, tìm ngược lên từ thư mục hiện
   tại. Một manifest không nói gì về PHP thì không phải câu trả lời cho PHP, nên pin ở lớp ngoài
   vẫn có hiệu lực.
3. Project đã đăng ký có gốc là thư mục này hoặc một thư mục cha.
4. Giá trị mặc định toàn cục.

Thay vì tự tính, hãy hỏi:

```bash
mix runtime resolve php
```

Lệnh này trả lời thư mục hiện tại dùng phiên bản nào đã cài, **và nguồn nào trong bốn nguồn trên
quyết định điều đó**. Vế sau mới là thứ người ta thật sự muốn biết khi một phiên bản khiến họ ngạc
nhiên. Không có gì được chạy để tìm ra câu trả lời.

## Giữ project luôn sẵn sàng

Khi bật *Tiết kiệm pin*, service không ai dùng trong một lúc sẽ được tạm dừng (xem trang services).
Trong lúc bạn đang làm việc với một project thì đây là điều bạn không muốn:

```bash
mix project keep-warm blog
mix project keep-warm blog --off
```

Đây là một lệnh riêng chứ không phải một thuộc tính của project, vì đó là việc bạn làm trong một
buổi chiều chứ không phải một phần của bản chất project. Lệnh này tác động tới pool PHP mà các site
của project dùng. Nó chưa tác động tới cơ sở dữ liệu mà project truy vấn, vì MixLab chưa ghi lại
project nào dùng cơ sở dữ liệu nào. Khi *Tiết kiệm pin* đang tắt thì không có gì bị tạm dừng, nên
lệnh này không thay đổi gì.
