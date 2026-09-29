+++
title = "Tham chiếu lệnh"
slug = "cli"
order = 15
summary = "Danh sách đầy đủ mọi lệnh mix, sinh tự động từ chính chương trình. Chỉ có bản tiếng Anh, và đây là lý do."
untranslated_reason = "The reference is generated from the binary's own English help strings; a hand-translated copy would be a second source of truth for twenty commands, drifting in silence."
+++

# Tham chiếu lệnh

> **Đây là tài liệu hướng dẫn dùng MixLab qua dòng lệnh `mix`.** Nếu bạn muốn thao tác bằng
> giao diện đồ hoạ cho dễ hơn thì bạn đã có sẵn: mọi bộ cài đều đặt sẵn cửa sổ MixLab ngay cạnh
> dòng lệnh. Cả hai đều điều khiển cùng một MixEngine bên dưới, nên mọi khái niệm trong cẩm nang
> này vẫn áp dụng.

Đây là trang duy nhất trong cẩm nang **không** có bản tiếng Việt, và đó là cố ý.

Bản tham chiếu lệnh không do ai viết ra. Nó được **sinh tự động** từ chính chương trình `mix`: mỗi
lệnh, mỗi cờ và mỗi câu mô tả đều được đọc thẳng từ định nghĩa mà chương trình dùng để phân tích
dòng lệnh của bạn. Nhờ vậy nó không thể mô tả một cờ không tồn tại, và cũng không thể bỏ sót một cờ
vừa được thêm vào.

Những định nghĩa đó viết bằng tiếng Anh, vì `mix --help` và `mix <lệnh> --help` trả lời bằng tiếng
Anh. Một bản dịch tay của trang này sẽ thành nguồn sự thật thứ hai cho hai mươi lệnh cùng các lệnh
con của chúng. Mà nguồn thứ hai thì lệch đi trong im lặng: nó vẫn trông đúng rất lâu sau khi chương
trình đã thay đổi.

## Đọc bản tham chiếu ở đâu

Bản tiếng Anh nằm tại `https://mixnz.github.io/mixlab/en/cli/`, và cũng có sẵn ngay trong chương
trình:

```bash
mix docs cli
mix docs --reference
```

Lệnh đầu in trang đó ra. Lệnh thứ hai in nội dung gốc dùng để sinh ra trang đó.

Trên máy của bạn, `--help` luôn là câu trả lời sát nhất và mới nhất:

```bash
mix --help
mix site --help
mix site create --help
```

## Các trang còn lại thì sao

Mọi trang khác của cẩm nang đều có bản tiếng Việt đầy đủ, và mỗi bản dịch đều ghi lại nó được dịch
từ phiên bản tiếng Anh nào. Sửa trang tiếng Anh mà không xem lại bản tiếng Việt là một lỗi
test, không phải chuyện ai đó tình cờ phát hiện nửa năm sau. Bắt đầu từ
[trang chủ của cẩm nang](./index.md).
