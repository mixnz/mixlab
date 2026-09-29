+++
title = "Cho điện thoại xem site của bạn"
slug = "sharing"
order = 8
summary = "Đưa đúng một site ra mạng nội bộ, quét mã QR, rồi rút nó về. Một site, một cổng, một luật."
translation_of = "en/sharing.md"
source_sha256 = "b1408dbb04f9463fb4a7fbb5f1ea870538882fe3171a07166e2a8b3dafa1c2a6"
+++

# Cho điện thoại xem site của bạn

> **Đây là tài liệu hướng dẫn dùng MixLab qua dòng lệnh `mix`.** Nếu bạn muốn thao tác bằng
> giao diện đồ hoạ cho dễ hơn thì bạn đã có sẵn: mọi bộ cài đều đặt sẵn cửa sổ MixLab ngay cạnh
> dòng lệnh. Cả hai đều điều khiển cùng một MixEngine bên dưới, nên mọi khái niệm trong cẩm nang
> này vẫn áp dụng.

Mọi thứ MixLab phục vụ đều chỉ trả lời trên loopback, không ở đâu khác. Muốn thử trên điện thoại
thật thì phải tạo một ngoại lệ, và ngoại lệ này áp dụng cho từng site, do bạn chủ động, và có thể
rút lại.

```bash
mix site share blog.test
```

Lệnh này in ra một URL mà điện thoại mở được, và một mã QR để bạn giơ camera vào. Ba việc đã xảy
ra:

1. Site bắt đầu trả lời trên địa chỉ của máy này trong mạng nội bộ, thay vì chỉ trên loopback.
   **Chỉ site này thôi.** Mọi site khác vẫn chỉ trả lời trên loopback.
2. Chứng chỉ được cấp lại để bao cả địa chỉ đó, nên ổ khoá vẫn còn khi truy cập từ máy khác.
3. Một hộp thoại quản trị xin thêm một rule firewall, cho đúng một cổng đó.

## Khi máy có nhiều mạng

MixLab **từ chối tự chọn** thay vì đưa site của bạn lên một mạng bạn không định đưa. Trường hợp
điển hình là laptop đang vừa nối Wi-Fi văn phòng vừa bật VPN. Lệnh liệt kê các mạng có thể chọn,
và bạn tự chỉ định:

```bash
mix site share blog.test --interface "Wi-Fi"
```

## Chia sẻ có thời hạn

```bash
mix site share blog.test --for 2h
```

Chấp nhận `30s`, `90m`, `2h`, `1d`, hoặc một số giây không đơn vị. Thời hạn tính **từ lúc bắt đầu
chia sẻ**, nên nếu bạn đặt thời hạn ngắn hơn khoảng thời gian site đã được chia sẻ thì lệnh bị từ
chối, chứ không kết thúc chia sẻ ngay lập tức.

Không có `--for` thì chia sẻ kéo dài tới khi bạn tự kết thúc, hoặc tới khi máy này rời khỏi mạng
đã chia sẻ. Trường hợp sau đáng để biết: gập laptop lại rồi mở ở chỗ khác sẽ kết thúc chia sẻ, vì
địa chỉ đã dùng để chia sẻ không còn là địa chỉ của máy này nữa.

## Rút về

```bash
mix site unshare blog.test
```

Lệnh này gỡ rule firewall, bind site lại về loopback, và cấp lại chứng chỉ không còn địa chỉ mạng.
Site chưa được chia sẻ thì giữ nguyên, nên chạy lệnh này khi không chắc cũng không mất gì.

## Cần biết trước khi dùng

- **Bất kỳ ai trong mạng đó đều truy cập được site.** Không có lớp xác thực nào phía trước. Ở mạng
  quán cà phê hay văn phòng chung, hãy chia sẻ có thời hạn, và rút về khi xong việc.
- **Chứng chỉ vẫn là của MixLab.** Điện thoại của bạn không tin CA của MixLab, nên nó sẽ cảnh
  báo. Chia sẻ là để kiểm tra layout trên màn hình thật, không phải để trình diễn ổ khoá.
- **Các site khác không đổi gì.** Luật là một cổng, một site, và nó được hoàn tác bằng `unshare`,
  khi hết thời hạn, hoặc khi rời khỏi mạng.

Mọi thứ hộp thoại xin quyền hỏi đều được liệt kê ở
[MixLab xin quyền để làm gì](./permissions.md).
