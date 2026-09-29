+++
title = "Quyền riêng tư"
slug = "privacy"
order = 17
summary = "MixLab giữ gì trên máy bạn, những lần hiếm hoi nó tự kết nối mạng, và máy chủ đồng bộ giữ gì khi bạn đăng nhập mà không đọc được gì."
translation_of = "en/privacy.md"
source_sha256 = "29588b3d68aca26935d0d4a38221bf2ddfbadd84a783c0a0f41be5a77fa9a43c"
+++

# Quyền riêng tư

Có hiệu lực từ 21 tháng 9 năm 2026 · Cập nhật lần cuối 21 tháng 9 năm 2026

**MixLab không thu thập bất cứ thông tin nào về bạn.** Điều này đúng với toàn bộ MixLab: dòng lệnh
`mix`, daemon MixEngine và cửa sổ MixLab. Không phần nào chứa mã phân tích, telemetry, báo lỗi hay
quảng cáo.

Dịch vụ duy nhất của chúng tôi là **đồng bộ**, và nó tắt cho tới khi bạn đăng nhập. Những gì nó
mang đi được mã hoá trên máy bạn trước khi rời máy, bằng khoá mà máy chủ không bao giờ nhận được,
nên máy chủ giữ dữ liệu của bạn mà không đọc được. Phần còn lại của trang này nói chính xác cái gì
được giữ ở đâu.

## Chúng tôi là ai

MixLab do mixnz (Nguyễn Hải Quang), Cẩm Mỹ, Đồng Nai, Việt Nam phát triển và phát hành. Câu hỏi về
chính sách này, hay về quyền riêng tư trong MixLab, xin gửi tới haiquang9994@outlook.com.

## Những gì nằm lại trên máy bạn

- **Home của MixEngine** (`mix status` cho biết nó ở đâu) chứa các runtime, dịch vụ, site, chứng
  chỉ và log mà MixEngine quản lý. Không thứ nào trong đó được gửi đi.
- **Cửa sổ MixLab** giữ cài đặt, kết nối đã lưu, request REST và lịch sử, bản nháp và snippet truy
  vấn dưới dạng file trong thư mục dữ liệu của ứng dụng, tên là `io.github.mixnz.mixlab`, ở vị trí
  quen thuộc của hệ điều hành.
- **Mật khẩu và bí mật** không được ghi vào các file đó. Chúng được giao cho kho thông tin đăng nhập
  mà hệ điều hành đã có sẵn (Credential Manager trên Windows, Keychain trên macOS, Secret Service
  trên Linux), dưới tên `MixLab`. Chúng không bao giờ được ghi vào log, và bản in gỡ lỗi hiện chúng
  thành `***`.

## Khi MixLab tự kết nối mạng

Ngoài những trường hợp dưới đây, MixLab im lặng trừ khi bạn bảo nó kết nối tới đâu đó.

- **Kiểm tra bản cập nhật.** Daemon đọc
  `https://github.com/mixnz/mixlab/releases/latest/download/latest.json` khi khởi động và mỗi ngày
  một lần. Yêu cầu này không mang định danh nào và không mang gì về bạn hay công việc của bạn.
- **Package và extension.** Danh mục những gì cài được, và những gì bạn cài từ đó, lấy từ
  `github.com/mixnz/mixengine-packages`.
- **Công cụ cơ sở dữ liệu.** Dump và restore cần công cụ của chính nhà cung cấp. Chỉ khi bạn yêu
  cầu, MixLab mới tải chúng từ `dev.mysql.com`, `get.enterprisedb.com`, `downloads.mongodb.org` và
  `fastdl.mongodb.org`.

GitHub và các nhà cung cấp đó thấy địa chỉ IP và user agent của yêu cầu, như mọi máy chủ web, theo
chính sách quyền riêng tư của riêng họ. Chúng tôi không nhận được gì từ họ.

**Kết nối do bạn tự tạo**, tới một cơ sở dữ liệu, qua SSH hay một request REST, đi thẳng từ máy bạn
tới địa chỉ bạn nhập. Không có gì của chúng tôi nằm giữa.

## Đồng bộ

Đồng bộ tắt cho tới khi bạn đăng nhập, và sau đó mọi loại dữ liệu vẫn tắt cho tới khi bạn bật nó
trong Settings. Máy chủ mặc định là `https://sync-0.lab.mixnz.com`, do chúng tôi vận hành trên
Cloudflare Workers.

**Những gì máy chủ giữ:**

- địa chỉ email của bạn, vì đó là tên đăng nhập và là nơi nhận mã;
- một giá trị băm của địa chỉ đó, dùng để đặt tên cho tài khoản;
- một *verifier* suy ra từ mật khẩu, không bao giờ là chính mật khẩu;
- tên bạn đặt cho các máy, và lần cuối mỗi máy được thấy;
- các bản ghi của bạn dưới dạng bản mã, cùng kích thước và thời điểm chúng thay đổi.

**Những gì nó không thấy được:** bất cứ gì bên trong một bản ghi, tên của bất kỳ loại dữ liệu nào,
một host, một URL, một tên file hay một mật khẩu. Khoá để mở một bản ghi chỉ tồn tại trên các máy
của bạn.

**Ai khác có liên quan.** Mã xác nhận và mã đặt lại được gửi qua **Mailtrap**, bên thấy được địa chỉ
của bạn và nội dung thư. **Cloudflare** chạy máy chủ và giữ log request tiêu chuẩn của họ, gồm cả
địa chỉ IP, theo chính sách quyền riêng tư của Cloudflare; chúng tôi chỉ đọc chúng để sửa lỗi. Để
chặn lạm dụng, máy chủ đếm số request theo địa chỉ IP và theo tài khoản, và quên mỗi con số sau một
giờ.

**Máy chủ bạn tự thêm** thuộc về người vận hành nó, không phải chúng tôi, và mục này không nói về
nó.

## Lưu giữ và xoá

- Một bản ghi bạn xoá để lại một dấu trong 90 ngày, để các máy khác của bạn biết nó đã mất; sau đó
  dấu ấy cũng mất.
- Gỡ một máy khỏi tài khoản sẽ đăng xuất máy đó ngay lập tức.
- Xoá tài khoản sẽ xoá hẳn tài khoản và mọi bản ghi, không để lại gì gọi tên địa chỉ của bạn.
- Đặt lại mật khẩu đã quên mà không có khoá khôi phục sẽ xoá mọi bản ghi, vì không còn gì đọc được
  chúng.

## Trẻ em

MixLab là công cụ cho lập trình viên. Nó không hướng tới trẻ em, và chúng tôi không cố ý thu thập
dữ liệu cá nhân của bất kỳ ai.

## Quyền của bạn

Các luật bảo vệ dữ liệu như GDPR và CCPA cho bạn quyền truy cập, sửa, xuất và xoá dữ liệu của mình.
Với đồng bộ, Settings của MixLab cho thấy những máy nào đang đăng nhập và gỡ được chúng; những gì nó
chưa làm được, kể cả xoá tài khoản, xin gửi tới địa chỉ ở trên. Mọi thứ khác đều nằm trên máy bạn
và trong tay bạn: home của MixEngine, thư mục dữ liệu của ứng dụng, và các mục tên `MixLab` trong
kho thông tin đăng nhập.

## Thay đổi

Mỗi lần trang này thay đổi, ngày ở đầu trang được cập nhật. Toàn bộ lịch sử của nó nằm trong
repository này.
