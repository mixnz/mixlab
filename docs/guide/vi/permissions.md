+++
title = "MixLab xin quyền để làm gì"
slug = "permissions"
order = 11
summary = "Mọi hộp thoại quyền quản trị mà MixLab có thể hiện lên, mỗi cái thay đổi chính xác điều gì, và vì sao không có gì của MixLab chạy thường trực với quyền root."
translation_of = "en/permissions.md"
source_sha256 = "70689e3d687acbe7aeea42f215b3c6b1c5ec9b53da08859822f9e1b4de56e77b"
+++

# MixLab xin quyền để làm gì

> **Đây là tài liệu hướng dẫn dùng MixLab qua dòng lệnh `mix`.** Nếu bạn muốn thao tác bằng
> giao diện đồ hoạ cho dễ hơn thì bạn đã có sẵn: mọi bộ cài đều đặt sẵn cửa sổ MixLab ngay cạnh
> dòng lệnh. Cả hai đều điều khiển cùng một MixEngine bên dưới, nên mọi khái niệm trong cẩm nang
> này vẫn áp dụng.

Một môi trường phát triển cục bộ buộc phải đụng vào vài thứ thuộc về cả máy: tên `blog.test` phải
phân giải được, trình duyệt phải tin một chứng chỉ, phải có gì đó lắng nghe trên cổng 80. Luật của
MixLab về tất cả những chuyện này rất ngắn.

**Không có gì MixLab chạy ở lại máy bạn với quyền quản trị.** Daemon không, web server không, cơ
sở dữ liệu cũng không. Khi thật sự cần một thay đổi đặc quyền, một chương trình nhỏ tách riêng tên
là `mixengine-elevate` được khởi động qua chính hộp thoại xin quyền của hệ điều hành, thực hiện
đúng thay đổi đó, rồi thoát. Nó không bao giờ chạy lệnh do ai đó đưa vào. Nó chỉ biết một số ít
thao tác được phép làm, và tự kiểm tra từng yêu cầu thay vì tin daemon đã gửi yêu cầu đó.

## Các hộp thoại, và mỗi cái thay đổi gì

Có sáu loại, và thường bạn chỉ gặp mỗi loại một lần.

### Định tuyến tên miền

Để `blog.test` và mọi tên bên dưới nó trỏ về máy bạn. MixLab chạy một DNS server nhỏ trả lời
`127.0.0.1` cho mọi tên dưới một hậu tố được quản lý. Việc cần xin quyền là trỏ hệ thống của bạn
vào server đó: một file dưới `/etc/resolver/` trên macOS, một rule resolver trên Linux, một rule
NRPT trên Windows.

Việc này chỉ hỏi **một lần**, không phải mỗi site một lần, và đó là toàn bộ lý do DNS server tồn
tại. Nếu dùng cách sửa file hosts thì mỗi lần tạo site lại phải nhập mật khẩu. Ở đâu không dùng
được đường resolver, MixLab chuyển sang ghi đúng một dòng cho mỗi tên trong file hosts, trong một
khối được đánh dấu rõ mà nó sở hữu và có thể gỡ đi.

### Tin cậy certificate authority

MixLab tự cấp chứng chỉ để site của bạn chạy `https://` mà không có cảnh báo. Để trình duyệt
chấp nhận, CA đã ký các chứng chỉ đó phải nằm trong trust store của hệ thống, và đưa nó vào đó cần
xin quyền.

**Điều này có nghĩa gì, và không có nghĩa gì.** CA được tạo trên máy bạn và khoá bí mật của nó
không bao giờ rời khỏi máy. Nó có thể chứng nhận cho bất kỳ tên nào, nên bạn nên hiểu rằng cài nó
là một quyết định tin cậy thật sự. Mọi công cụ HTTPS cục bộ đều xin đúng quyền này. Từ chối cũng
được: site của bạn vẫn chạy qua `http://`, và MixLab nói rõ điều đó thay vì báo lỗi.

Trên Linux, Chrome và Firefox đọc cơ sở dữ liệu chứng chỉ riêng của chúng thay vì store hệ thống,
nên MixLab ghi vào đó nữa. Việc này không cần quyền quản trị, vì các file đó là của bạn.

### Lắng nghe trên cổng 80 và 443

Trên macOS và Linux, cổng dưới 1024 là cổng đặc quyền. MixLab không giải quyết bằng cách chạy
web server dưới quyền root. Nó cấp khả năng đó cho đúng một chương trình cần nó và không gì khác,
rồi server chạy dưới tài khoản của bạn.

### Rule firewall, khi bạn chia sẻ site

Chỉ khi bạn muốn một site truy cập được từ điện thoại hoặc máy khác trong cùng mạng. Rule chỉ mở
đúng một cổng đó, được gỡ khi bạn ngừng chia sẻ, và không đụng gì khác trong firewall của bạn.

### Cài chương trình phụ trợ đặc quyền

Bản thân `mixengine-elevate` phải nằm ở nơi bạn không ghi được. Một chương trình chạy với quyền
quản trị mà nằm trong thư mục bất kỳ tiến trình nào cũng ghi đè được thì không phải ranh giới bảo
mật. Vì thế việc đặc quyền đầu tiên MixLab làm là đặt chương trình phụ trợ này vào chỗ. Hai
cách cài MixLab chạy hoàn toàn dưới tài khoản của bạn: bộ cài Windows và build từ mã nguồn. Đó là
lý do việc này không thể là việc của bộ cài. Ở đâu `.deb`, `.rpm` hoặc
`.pkg` đã đặt sẵn nó, MixLab nhận ra và không hỏi gì.

### Thay chương trình phụ trợ đặc quyền

Bạn không bao giờ phải tự làm việc này. Chương trình phụ trợ có số phiên bản riêng, chỉ đổi khi
chính nó thay đổi, và chuyện đó hiếm. Khi MixLab khởi động và thấy bản đang cài cũ hơn, nó thay
bản đó ở lần xin quyền kế tiếp, là lần MixLab vốn cũng phải hỏi. Sau một bản cập nhật có đổi
chương trình phụ trợ, MixLab tự hỏi quyền một lần.

Chương trình phụ trợ đang cài tự kiểm tra chữ ký của MixLab trên bản thay thế trước khi cho phép
ghi đè lên chính nó. Bản quá cũ, không làm được việc đó, thì được thay bằng bản đi kèm bản phát
hành này, giống như lúc cài lần đầu.

## Một hộp thoại, không phải sáu

MixLab gom những việc cần xin quyền lại và hỏi một lần. Trên máy mới, tạo site HTTPS đầu tiên
thường chỉ hiện một hộp thoại duy nhất, gồm cả rule resolver, CA và quyền dùng cổng.

Bạn có thể xem hàng đợi trước khi bất cứ gì được hỏi:

```bash
mix elevation status
```

Lệnh này in ra mọi thao tác đang chờ và nó sẽ thay đổi gì: chính xác các dòng hosts, cổng nào,
store nào. Rồi khi bạn sẵn sàng:

```bash
mix elevation grant
```

**Nói không là câu trả lời bình thường.** Hàng đợi vẫn ở nguyên đó, không có gì bị áp dụng nửa
chừng, và bạn có thể chạy lại lệnh sau. Nếu bạn quyết định một thao tác không bao giờ cần hỏi lại
nữa:

```bash
mix elevation drop <id>
```

## Nhật ký kiểm tra

Chương trình phụ trợ ghi một dòng cho mỗi thao tác đặc quyền nó thực hiện, vào một file mà chỉ
quản trị viên mới sửa được:

| Hệ điều hành | Đường dẫn |
| --- | --- |
| Windows | `%ProgramData%\MixEngine\elevate.log` |
| macOS | `/Library/Logs/MixEngine/elevate.log` |
| Linux | `/var/log/mixengine/elevate.log` |

File đó và bản thân chương trình phụ trợ là hai thứ duy nhất MixLab để lại bên ngoài thư mục
riêng của nó. `mix doctor` báo cáo cả hai và không xoá cái nào, vì nhật ký kiểm tra chính
là bằng chứng về thứ nó đang chẩn đoán.
`mix uninstall` mới là lệnh gỡ chúng đi, và nó sẽ hỏi trước.
