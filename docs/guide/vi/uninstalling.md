+++
title = "Gỡ MixLab"
slug = "uninstalling"
order = 13
summary = "Hoàn tác mọi thứ MixLab đã ghi bên ngoài thư mục của nó, xem danh sách trước khi đồng ý, và giữ lại cơ sở dữ liệu nếu bạn muốn."
translation_of = "en/uninstalling.md"
source_sha256 = "c06aa2e236ad0d7d2be0e1d9c2a5f56d4cfe13be2200de376a906d70feb241f4"
+++

# Gỡ MixLab

> **Đây là tài liệu hướng dẫn dùng MixLab qua dòng lệnh `mix`.** Nếu bạn muốn thao tác bằng
> giao diện đồ hoạ cho dễ hơn thì bạn đã có sẵn: mọi bộ cài đều đặt sẵn cửa sổ MixLab ngay cạnh
> dòng lệnh. Cả hai đều điều khiển cùng một MixEngine bên dưới, nên mọi khái niệm trong cẩm nang
> này vẫn áp dụng.

MixLab ghi gần như mọi thứ vào một thư mục duy nhất. Ngoại lệ là vài thay đổi đặc quyền mà nó
đã xin phép bạn, và `mix uninstall` chính là để thu hồi những thay đổi đó.

## Trên Windows, Installed apps lo hết

Gỡ MixLab trong Installed apps. Bộ gỡ hỏi hai lựa chọn, mặc định đều không tick:

- **Also delete MixLab's data**: thư mục home, gồm database, chứng chỉ và bản ghi các project.
- **Also delete the folders you moved out of it**: chỉ hiện khi bạn đã dùng `[paths]` để chuyển
  `runtimes`, `packages`, `data` hoặc `logs` sang chỗ khác, và liệt kê các thư mục đó.

Nếu MixLab đang mở, bộ gỡ hỏi rồi đóng nó. Sau đó nó kiểm tra mọi thứ có thể làm kẹt giữa chừng:
file đang được dùng, hoặc chương trình đang chạy từ thư mục của MixLab, ví dụ một `php` bạn chạy
qua shim. Có thứ nào chặn thì nó nêu tên và không xoá gì cả. Tắt thứ đó rồi bấm Uninstall lại.

Tiếp theo là một hộp thoại quản trị để hoàn tác các thay đổi trên máy liệt kê bên dưới, rồi đến
lượt chương trình. Nếu bạn từ chối hộp thoại, không có gì bị xoá và MixLab vẫn còn nguyên. Khi nào
sẵn sàng thì chạy Uninstall lại.

## Trên Mac, MixLab tự lo hết

Nếu bạn cài MixLab bằng file `.pkg`, chọn **MixLab ▸ Gỡ MixLab khỏi máy Mac này…** (giao diện
tiếng Anh ghi *Remove MixLab from this Mac…*). MixLab liệt kê mọi thứ nó sẽ trả lại, và mọi thứ
đang chặn, ví dụ một `node` bạn chạy qua shim. Tắt thứ đó rồi bấm **Kiểm tra lại**. Nó hỏi hai lựa
chọn giống bộ gỡ trên Windows, mặc định đều không tick.

Bấm **Gỡ MixLab** thì bạn nhập mật khẩu một lần. Xong việc, MixLab tự thoát và không để lại gì:
không còn lệnh nào trong `/usr/local/bin`, không còn ứng dụng, không còn receipt của gói. Nếu bạn
từ chối hộp thoại, không có gì bị xoá và mục menu vẫn còn đó.

Phần còn lại của trang này là cùng việc đó nhưng làm bằng `mix`, cũng là cách làm trên Linux, hoặc
trên Mac từ terminal.

## Xem danh sách trước

```bash
mix uninstall --dry-run
```

Lệnh này không thay đổi gì, chỉ liệt kê từng thứ nó sẽ gỡ:

- khối trong file hosts, và rule DNS hoặc resolver dùng để định tuyến tên miền của bạn
- quyền lắng nghe trên cổng 80 và 443
- certificate authority, khỏi mọi store đang tin nó
- rule firewall nào còn sót lại từ site đã chia sẻ
- mục tự khởi động daemon khi bạn đăng nhập
- mục trong `PATH`
- chương trình phụ trợ đặc quyền, cùng nhật ký kiểm tra của nó
- cache và log của cửa sổ MixLab, cùng các kết nối và lịch sử nó đã lưu nếu thư mục bên dưới
  cũng bị xoá
- mật khẩu database, cùng mật khẩu đã lưu và phiên đăng nhập sync của MixLab, trong kho mật
  khẩu của hệ thống. Nếu bạn giữ thư mục bên dưới thì chúng được giữ theo
- và cuối cùng là thư mục riêng của MixLab

## Gỡ

Thoát MixLab trước. Nếu cửa sổ còn mở, nó sẽ lưu lại mật khẩu ngay sau khi chúng bị xoá.

```bash
mix uninstall
```

Bạn sẽ được hỏi xác nhận, và một hộp thoại quản trị duy nhất bao trọn phần đặc quyền. `--yes` trả
lời trước câu xác nhận đó, dành cho script.

**Chưa có gì thay đổi cho tới khi bạn cho phép.** Từ chối hộp thoại thì `PATH`, mục tự khởi động
và các browser của bạn vẫn y như cũ. Khi nào sẵn sàng thì chạy lại lệnh.

**Có chương trình đang chạy từ thư mục của MixLab thì lệnh dừng ngay từ đầu.** Danh sách đánh dấu
nó là `BLOCKED` và lệnh thoát với mã `3`, để script phân biệt được "tắt cái này rồi thử lại" với
một lỗi thật. Tắt chương trình đó rồi chạy lại lệnh.

**Báo cáo là kết quả đo được, không phải lời khẳng định.** Thứ trả về là những gì MixLab tìm
thấy trên máy *sau khi* gỡ, từng dòng một, kể cả những dòng trả lời *không có gì ở đây*. Nếu báo
cáo giấu những dòng đó, bạn sẽ không phân biệt được "không có cấu hình resolver nào" với "chưa
kiểm tra cấu hình resolver". Lệnh thoát với mã khác không nếu bất cứ thứ gì nó đã xử lý vẫn còn,
để script kiểm tra được.

Kết nối sẽ đứt giữa chừng, và đó là bình thường: daemon đang xoá chính thư mục home nó phục vụ, nên
nó tự dừng. Sau đó MixLab đọc lại các dòng cuối trực tiếp từ đĩa. Nhờ vậy câu trả lời là *không
còn gì sót lại*, chứ không phải *daemon bảo thế*.

## Giữ lại dữ liệu

```bash
mix uninstall --keep-home
```

Lệnh này hoàn tác mọi thứ **bên ngoài** thư mục home và để nguyên home: cơ sở dữ liệu trong `data/`,
chứng chỉ, bản ghi các project. Xong việc thì daemon dừng, và lần cài sau sẽ dùng lại home này.

Nếu bạn đã dùng `[paths]` để chuyển `runtimes`, `packages`, `data` hoặc `logs` sang ổ khác, các thư
mục đó là một lựa chọn riêng:

```bash
mix uninstall --keep-relocated
```

giữ chúng lại và xoá home. Dùng cả hai cờ để giữ tất cả. Thư mục nào bạn chưa từng chuyển đi thì
nằm trong home, nên đi cùng home.

### Cài lại trên các thư mục đã giữ

Khi bản cài mới hỏi nơi lưu dữ liệu, chọn đúng các thư mục cũ. Lúc khởi động:

- **Project, site và service** quay lại từ bản sao mà lần gỡ để lại trong các thư mục đó: Dashboard
  hiện *Khôi phục lần cài trước*, hoặc chạy `mix home restore`. Database được đặt mật khẩu quản trị
  mới và để ở trạng thái dừng cho đến khi bạn bật.

- **Runtime và package** có sẵn trong đó tự hiện là đã cài. Bản nào MixLab chưa kiểm được, ví dụ vì
  lúc đó không có mạng, sẽ nằm trong mục *Có trên đĩa, chưa ghi nhận* ở màn Runtimes, kèm nút
  **Nhận lại**. Từ terminal: `mix runtime found`, rồi `mix runtime adopt php 8.3.33` (với package
  cũng vậy).
- **Database** trong `data/` hiện trên Dashboard dưới dạng *Dữ liệu service từ lần cài trước*. Bấm
  *Xem* rồi **Nhận lại**. Service quay lại ở trạng thái dừng, với mật khẩu quản trị mới, và mọi
  database cùng tài khoản bên trong vẫn giữ nguyên. Từ terminal: `mix service found`, rồi
  `mix service adopt mariadb@main`.

Tài khoản database riêng của từng app vẫn dùng mật khẩu mà app đang giữ. Chỉ mật khẩu quản trị là
mới, vì mật khẩu cũ đã mất cùng home.

## Rồi gỡ chính chương trình

`mix uninstall` gỡ những gì MixLab đã làm. Còn bản thân chương trình thì tuỳ cách bạn đã cài.

Trên Mac, thêm `--package` là cùng hộp thoại quản trị đó gỡ luôn chương trình: các lệnh trong
`/usr/local/bin`, `MixLab.app` và receipt của gói.

```bash
mix uninstall --package
```

Trên Linux, trình quản lý gói lo phần này, và `mix uninstall` in sẵn lệnh khi chạy xong:

```bash
sudo apt remove mixlab
sudo dnf remove mixlab
```

Trên Windows, bộ gỡ của bộ cài đã làm luôn phần này.

Một máy Mac không có ai đăng nhập ở màn hình thì không hiện được hộp thoại quản trị. Khi đó, gỡ
chương trình bằng tay:

```bash
sudo rm -rf /usr/local/bin/mix /usr/local/bin/mixengined /usr/local/bin/mixengine-shim \
  /usr/local/bin/mixengine-trampoline /Applications/MixLab.app
sudo pkgutil --forget dev.mixengine.cli
```

## Những gì cố ý không tự động

Nhật ký kiểm tra của chương trình phụ trợ đặc quyền thuộc sở hữu root, và bản thân chương trình đó
cũng vậy. `mix doctor` báo cáo cả hai và không xoá cái nào, vì nhật ký chính là bằng chứng
về thứ nó đang chẩn đoán. `mix uninstall` mới là
lệnh gỡ chúng, và nó sẽ hỏi trước.
