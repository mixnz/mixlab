+++
title = "Giữ MixLab luôn mới"
slug = "updating"
order = 12
summary = "Cập nhật do bạn quyết, có kiểm tra chữ ký, và có chạy thử trước khi thay bất cứ thứ gì. Riêng một chương trình cố ý không bao giờ được thay theo đường này."
translation_of = "en/updating.md"
source_sha256 = "dbab2abe75a44397cbc60778ef9ff366c6fab06db85c1cf248515ca21008778e"
+++

# Giữ MixLab luôn mới

> **Đây là tài liệu hướng dẫn dùng MixLab qua dòng lệnh `mix`.** Nếu bạn muốn thao tác bằng
> giao diện đồ họa cho dễ hơn thì bạn đã có sẵn: mọi bộ cài đều đặt sẵn cửa sổ MixLab ngay cạnh
> dòng lệnh. Cả hai đều điều khiển cùng một MixEngine bên dưới, nên mọi khái niệm trong cẩm nang
> này vẫn áp dụng.

```bash
mix self-update --check
mix self-update
```

`--check` in ra bản có sẵn, gồm phiên bản, dung lượng và những gì đã thay đổi, và không cài gì.
Không có `--check` thì cũng hiện đúng thông tin đó, rồi hỏi bạn có cập nhật không.

## Cập nhật không bao giờ âm thầm

Cập nhật sẽ khởi động lại các service bạn đang chạy. Vì thế đó là việc bạn chọn, không phải việc
xảy ra với bạn giữa lúc đang làm, nên **không có gì được cài mà không hỏi**. Daemon có kiểm tra
lặng lẽ, lúc khởi động và mỗi ngày một lần, để `mix status` báo được cho bạn là có bản mới. Cả hai
lần kiểm tra đều thất bại trong im lặng nếu không có mạng: máy không có mạng không phải máy có vấn
đề.

`--yes` trả lời trước câu hỏi, dành cho script chạy khi không có ai ở bàn phím.

## Chuyện gì xảy ra khi bạn đồng ý

Theo thứ tự, và không bước nào bỏ qua được:

1. Bản phát hành được tải về và đối chiếu hash với feed cập nhật **có chữ ký**. Payload không khớp
   thì không được giải nén.
2. Chữ ký được kiểm tra bằng khóa công khai biên dịch sẵn trong MixEngine. Không có gì ở tầng
   truyền tải được tin để quyết định một file có phải của chúng tôi hay không.
3. **Bản `mixengined` mới được chạy thử một lần** trước khi thay bất cứ gì, để chắc máy này khởi
   động được nó. Một bản cập nhật sẽ để lại daemon không chạy được thì bị chặn ở đây, thay vì phát
   hiện ra sau.
4. Những gì đang chạy được dừng, các file thực thi được thay, và daemon thoát.
5. `mix` khởi động daemon mới, và daemon khởi động lại service của bạn.

## Chương trình phụ trợ đặc quyền

`mixengine-elevate` chạy với quyền quản trị, nên thay nó cần bạn cho phép. Nó có số phiên bản
riêng, chỉ đổi khi chính nó thay đổi, nên phần lớn các bản cập nhật để nguyên nó và không hỏi gì.

Khi một bản cập nhật có đổi nó, daemon mới hỏi quyền một lần, ngay lần khởi động đầu tiên. Chương
trình phụ trợ đang cài tự kiểm tra chữ ký của MixLab trên bản thay thế trước khi cho phép ghi đè
lên chính nó. Nếu bạn từ chối thì không hỏng gì: bản cũ vẫn phục vụ mọi việc nó biết, và bản thay
thế đi kèm lần xin quyền kế tiếp mà MixLab vốn cũng phải hỏi.

## Khi bạn cài MixLab trên Mac bằng file `.pkg`

`mix self-update` vẫn cập nhật được, theo đúng cách file `.pkg` đã cài. Nó tải file `.pkg` mới,
kiểm tra với bản phát hành đã ký, rồi mở bằng Installer.app. Trong lúc bạn cài, không có gì bị dừng,
và bấm Cancel cũng không mất gì.

Cài xong thì chạy `mix self-update --finish`, hoặc bấm **Hoàn tất cập nhật** trong MixLab. MixEngine
khởi động lại ở bản mới và chạy lại các service đang chạy trước đó.

Nếu bạn SSH vào máy, Installer hiện trên màn hình của chính máy Mac đó. `mix self-update` in
thêm đường dẫn file đã tải và lệnh `sudo installer` để cài file đó từ terminal.

Bản đầu tiên có tính năng này thì vẫn phải cài tay một lần, như mọi file `.pkg`.

## Trên Linux

Bản cài từ `.deb` hoặc `.rpm` thuộc về `apt` hoặc `dnf`, nên MixLab không tự thay file của nó.
`mix self-update` tải gói tiếp theo cùng loại, kiểm tra với bản phát hành đã ký, rồi in lệnh cài:

```bash
sudo apt install '<đường dẫn nó in ra>/mixlab_0.0.10-1_amd64.deb'
```

Trên máy có desktop, nó mở luôn gói đó trong trình quản lý phần mềm. Cài xong thì chạy
`mix self-update --finish`.

Bộ cài Windows được `mix self-update` cập nhật tại chỗ.

## Phiên bản

MixLab dùng semantic versioning, một số phiên bản chung cho mọi thứ nó phát hành. Trước 1.0, API
có thể thay đổi không tương thích giữa các phiên bản minor, và mỗi thay đổi như vậy được liệt kê
trong changelog. Đó chính là thứ `mix self-update --check` in ra trước khi hỏi bạn.
