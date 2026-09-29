+++
title = "Extension"
slug = "extensions"
order = 10
summary = "Những công cụ đi kèm stack như phpMyAdmin, Mailpit, MinIO. Cài từ một registry có chữ ký, và cho bạn xem mỗi cái được phép làm gì trước khi đồng ý."
translation_of = "en/extensions.md"
source_sha256 = "dbc8da97a5411b73dac121c57bb832e7eb87d946f6f5e96b03b10f3c4af70f48"
+++

# Extension

> **Đây là tài liệu hướng dẫn dùng MixLab qua dòng lệnh `mix`.** Nếu bạn muốn thao tác bằng
> giao diện đồ hoạ cho dễ hơn thì bạn đã có sẵn: mọi bộ cài đều đặt sẵn cửa sổ MixLab ngay cạnh
> dòng lệnh. Cả hai đều điều khiển cùng một MixEngine bên dưới, nên mọi khái niệm trong cẩm nang
> này vẫn áp dụng.

Extension là công cụ nằm bên cạnh stack của bạn chứ không phải bên trong: giao diện quản trị cơ sở
dữ liệu, công cụ bắt mail, object store, search engine. MixLab cài nó, giám sát nó, và cấp cho
nó một tên miền cùng chứng chỉ, giống hệt cách làm với site của bạn.

## Có những gì

```bash
mix extension available
mix extension list
```

`available` là registry có chữ ký mà MixLab phát hành; `list` là những gì máy này đã cài.

Extension có ba dạng, và bạn nên nhận ra mình đang cài dạng nào:

| Dạng | Là gì |
| --- | --- |
| `web-app` | Mã nguồn chạy trên chính stack của bạn, ở một site nội bộ được sinh ra. Ví dụ phpMyAdmin, Adminer |
| `service` | Một chương trình MixLab giám sát như mọi service khác. Ví dụ Mailpit, MinIO, MeiliSearch |
| `recipe` | Chỉ có cấu hình: thêm directive cho web server, một profile `php.ini` |

## Xem trước khi cài

```bash
mix extension plan mailpit
```

Lệnh này không thay đổi gì, chỉ in ra việc cài sẽ tạo ra những gì: sẽ tải gì, tạo service nào,
truy cập ở site nào, và **nó xin được phép làm gì**.

Có hai dòng trong kế hoạch đáng đọc kỹ thay vì lướt qua, và chúng chỉ xuất hiện với dạng `web-app`:

- **Giao diện quản trị sẽ mở vào cơ sở dữ liệu nào.** Công cụ như phpMyAdmin chốt điều này lúc
  cài, và server nào nó quản trị không phải chi tiết để phát hiện về sau.
- **Nó sẽ đăng nhập bằng tài khoản nào.** Extension có thể khai báo rằng nó đăng nhập bằng tài khoản
  superuser của server, tức là thứ quyền hệ trọng nhất một extension có thể được cấp. Kế hoạch nêu
  tên tài khoản, nói rõ mật khẩu được lấy từ credential store của hệ điều hành khi pool khởi động,
  và không có gì ghi mật khẩu đó ra đĩa.

`mix extension install` hỏi bạn về tất cả những điều đó trước khi làm bất cứ gì. `--yes` bỏ qua câu
hỏi, dành cho script đã đọc kế hoạch rồi.

## Cài và gỡ

```bash
mix extension install mailpit
mix extension start mailpit
mix extension stop mailpit
mix extension uninstall mailpit
```

Cài là một job; `--no-wait` đưa bạn job id thay vì chờ.

`mix extension uninstall` **giữ lại dữ liệu của extension** trừ khi bạn nói khác, vì đó là lựa
chọn có thể hoàn tác. `--delete-data` là lựa chọn không hoàn tác được.

## Cài thứ không có trong registry

```bash
mix extension inspect ./my-tool
mix extension plan --path ./my-tool
mix extension install --path ./my-tool
```

`inspect` đọc file `extension.toml` và cho bạn biết nó khai báo gì, không cài gì cả.

**Không ai bảo đảm cho extension cài từ đường dẫn**, và bản ghi sẽ nói vậy chừng nào extension còn
được cài. Đây không phải cảnh báo bạn tắt đi được: chính nó làm cho extension chưa ký hiện rõ trong
mọi danh sách có nêu tên nó, để không ai phải nhớ nó đến từ đâu.

## Extension không phải là gì

**Extension không phải API client.** Nó không được gọi API của MixEngine, không được bảo daemon
thay đổi máy bạn, và không chạm tới thứ gì nó chưa khai báo. Nó chỉ nhận đúng những gì manifest đã
khai báo và bạn đã đồng ý: một cổng, một site, một service, một kết nối cơ sở dữ liệu. Không gì
khác.

Site của extension xuất hiện trong `mix site list` như mọi site khác, và có thể bật hoặc tắt. Mọi
chỉnh sửa khác lên site đó đều bị từ chối, và lời từ chối nêu tên lệnh uninstall để gỡ nó. Site
thuộc về extension, và sửa nó sau lưng extension sẽ làm hỏng extension.
