+++
title = "MixLab"
slug = "index"
order = 1
summary = "Chạy PHP, Node, Python, Ruby, Go và Java ngay trên máy với đúng phiên bản bạn cần, có tên miền thật và HTTPS, không cần Docker."
translation_of = "en/index.md"
source_sha256 = "ecde9ceea649df86b90b485ecf083feb198fdd13cd022315d9d45607a772cb3d"
+++

# MixLab

> **Đây là tài liệu hướng dẫn dùng MixLab qua dòng lệnh `mix`.** Nếu bạn muốn thao tác bằng
> giao diện đồ hoạ cho dễ hơn thì bạn đã có sẵn: mọi bộ cài đều đặt sẵn cửa sổ MixLab ngay cạnh
> dòng lệnh. Cả hai đều điều khiển cùng một MixEngine bên dưới, nên mọi khái niệm trong cẩm nang
> này vẫn áp dụng.

MixLab là môi trường phát triển web chạy ngay trên máy bạn. Bạn có thể cài nhiều phiên bản PHP,
Node.js, Python, Ruby, Go và Java cùng lúc, rồi để mỗi thư mục dự án tự chọn phiên bản mình dùng.
MixLab cũng chạy luôn web server, cơ sở dữ liệu và cache mà dự án cần, và cấp cho mỗi site một
tên miền thật như `https://blog.test` với chứng chỉ được trình duyệt tin cậy. Không Docker, không
máy ảo, không phải tự viết file cấu hình nào. Cấu hình sinh ra là việc của MixLab, và không có
tiến trình nào của MixLab chạy thường trực với quyền root.

MixEngine gồm một daemon và một lệnh. Daemon `mixengined` lưu mọi trạng thái và giám sát mọi tiến
trình MixEngine chạy. Còn `mix` là lệnh bạn gõ. Một vài thao tác cần quyền quản trị, ví dụ thêm một
dòng vào file hosts, đưa chứng chỉ vào trust store của hệ điều hành, hay xin quyền lắng nghe trên
cổng 80. Những thao tác này được gom lại hỏi một lần, và do một chương trình phụ trợ thực hiện rồi
thoát ngay khi xong việc.

## Bắt đầu ở đây

- [Cài đặt MixLab](./install.md): file cài cho hệ điều hành của bạn, và bộ cài đụng vào những gì
  trên máy.
- [Site đầu tiên của bạn](./getting-started.md): từ máy vừa cài xong tới ổ khoá xanh trên trình
  duyệt, mất khoảng năm phút.

## Cẩm nang

- [Dự án và site](./projects-and-sites.md): hai khái niệm cốt lõi, và cách một bản checkout mang
  theo cấu hình của chính nó.
- [Phiên bản PHP, Node, Python, Ruby, Go và Java](./runtimes.md): nhiều phiên bản cùng lúc, chọn
  theo từng thư mục.
- [Máy chủ, cơ sở dữ liệu và bộ nhớ đệm](./services.md): những thứ dự án của bạn cần để chạy.
- [Tên miền và ổ khoá](./domains-and-https.md): vì sao `blog.test` phân giải được, và ai ký chứng
  chỉ cho nó.
- [Cho điện thoại xem site của bạn](./sharing.md): mở một site ra mạng nội bộ, rồi đóng lại.
- [Blueprint](./blueprints.md): ghi lại dự án gồm những gì, để dựng lại y hệt ở máy khác.
- [Extension](./extensions.md): phpMyAdmin, Mailpit và các công cụ khác, cài từ một registry có
  chữ ký.
- [MixLab xin quyền để làm gì](./permissions.md): từng hộp thoại xin quyền, và mỗi cái thay đổi
  gì trên máy.
- [Cập nhật MixLab](./updating.md): cập nhật do bạn quyết, có kiểm tra chữ ký, có chạy thử
  trước.
- [Gỡ MixLab](./uninstalling.md): và cách kiểm tra xem còn sót lại gì không.
- [Khi có gì đó không ổn](./troubleshooting.md): chạy `mix doctor` trước đã.
- [Tham chiếu lệnh](./cli.md): mọi lệnh và mọi cờ, sinh tự động từ chính chương trình.

## Dành cho chương trình

- [Đọc cẩm nang này bằng chương trình](./for-agents.md): mọi trang ở dạng Markdown thuần, một
  manifest, và cùng nội dung đó khi offline qua `mix docs`.

Mọi trang ở đây đều có bản tiếng Anh và bản tiếng Việt, và đều được phát hành thêm dưới dạng
Markdown thuần tại một địa chỉ dễ đoán. Chính những trang này được biên dịch thẳng vào chương trình
`mix`, nên `mix docs` vẫn trả lời được trên máy không có mạng và không có daemon nào đang chạy.

MixLab chạy trên Windows, macOS và Linux, và mọi trang ở đây đúng cho cả ba. Chỗ nào một hệ điều
hành thật sự khác, trang đó sẽ nói rõ đang nói về hệ nào.
