+++
title = "Tên miền và ổ khoá"
slug = "domains-and-https"
order = 7
summary = "Vì sao blog.test trỏ về máy bạn, ai ký chứng chỉ cho nó, và cách tìm ra vấn đề khi ổ khoá không xanh."
translation_of = "en/domains-and-https.md"
source_sha256 = "a6b49830f6f2dfad251d86fe636b7dfe33fd79dfa42de6d2dc3705d79bea8ac5"
+++

# Tên miền và ổ khoá

> **Đây là tài liệu hướng dẫn dùng MixLab qua dòng lệnh `mix`.** Nếu bạn muốn thao tác bằng
> giao diện đồ hoạ cho dễ hơn thì bạn đã có sẵn: mọi bộ cài đều đặt sẵn cửa sổ MixLab ngay cạnh
> dòng lệnh. Cả hai đều điều khiển cùng một MixEngine bên dưới, nên mọi khái niệm trong cẩm nang
> này vẫn áp dụng.

Để `https://blog.test` mở lên không có cảnh báo, cần hai điều. Tên miền phải trỏ về đúng máy bạn,
và trình duyệt phải chấp nhận chứng chỉ mà server đưa ra. MixLab lo cả hai, và trang này nói về
những gì nó thật sự đã làm.

## Bạn được dùng những tên nào

| Hậu tố | Trạng thái |
| --- | --- |
| `.test` | **Mặc định.** Được tổ chức tiêu chuẩn dành riêng cho đúng mục đích này, không bao giờ phân giải được trên internet, và không thể trùng với thứ gì có thật |
| `.internal` | Cũng được quản lý. Dành riêng cho mục đích nội bộ, và nghe như một ý định thay vì một thử nghiệm như `.test` |
| `.localhost` | Lựa chọn không cần cấu hình: nhiều hệ thống đã sẵn trỏ `*.localhost` về loopback, nên không phải sửa gì |
| `.local` | Hỗ trợ, nhưng có cảnh báo. Xem bên dưới |
| `.dev`, `.app`, … | **Từ chối.** Đây là tên miền công cộng có thật, được trình duyệt ép dùng HTTPS; chiếm một tên như vậy trên máy sẽ làm hỏng internet thật của bạn |

**`.local` thuộc về mDNS**, là cơ chế để máy in và loa tự giới thiệu mình trên mạng. Dùng nó vẫn
chạy, cho tới khi ai đó cắm một thiết bị như vậy vào. MixLab vẫn cho phép, nhưng CLI bắt bạn
ghi rõ `--i-know`, và không bao giờ trỏ *resolver* vào `.local`. Site ở hậu tố này chỉ nhận đúng
một dòng trong file hosts, không hơn, vì nếu đẩy mọi tên `.local` về loopback thì mọi thiết bị
Bonjour trên mạng của bạn sẽ hỏng.

## Tên miền trỏ về máy bạn bằng cách nào

MixLab chạy một DNS server nhỏ của riêng nó, trả lời `127.0.0.1` cho **mọi** tên dưới một hậu
tố được quản lý, ở bất kỳ độ sâu nào, dù đã khai báo site cho tên đó hay chưa. Nhờ vậy
`api.blog.test` và `staging.blog.test` hoạt động được mà không cần ai khai báo.

Trỏ hệ thống của bạn vào server đó chỉ cần xin quyền **một lần**. Ngược lại, nếu dùng file hosts
thì mỗi lần tạo site lại phải nhập mật khẩu. Đó là toàn bộ lý do DNS server là cơ chế chính, còn
file hosts chỉ là phương án dự phòng. Ở đâu không dùng được đường resolver, MixLab ghi đúng một
dòng cho mỗi tên, trong một khối được đánh dấu mà nó sở hữu và có thể gỡ đi.

Truy vấn `AAAA` được trả lời là không có bản ghi, thay vì `::1`, và đây là cố ý: front end lắng
nghe trên IPv4. Một tên phân giải ra địa chỉ không ai lắng nghe sẽ khiến trình duyệt chờ một lúc
rồi mới thử địa chỉ khác.

## Thêm và bớt tên miền

```bash
mix domain add api.blog.test --site blog.test
mix domain remove api.blog.test
```

Tên thêm bằng cách này là **alias**. Tên miền chính của site không đổi, vì tên chính là thứ URL
chuẩn và chứng chỉ lấy tên theo. Không thể xoá tên miền cuối cùng của site, cũng không xoá được
tên chính. Muốn sắp xếp lại thì dùng `mix site update`; `--domain` đầu tiên bạn truyền sẽ thành
tên chính.

## Khi một tên miền không hoạt động

```bash
mix domain status blog.test
```

Đây là lệnh chẩn đoán nên dùng, và nó được thiết kế để chỉ ra từng phần hỏng thay vì chỉ nói
"hỏng". Nó trả lời bốn câu hỏi riêng biệt: tên đã được khai báo chưa, nó được định tuyến bằng cách
nào, hiện tại nó có thật sự phân giải được trên máy này không, và có gì đang trả lời ở đó không.
Không truyền tham số thì lệnh làm vậy cho mọi tên mà MixLab này biết.

## Certificate authority

MixLab tự cấp chứng chỉ thay vì dùng một CA công cộng, vì tên miền cục bộ không phân giải được
trên internet và không CA công cộng nào chịu ký cho chúng. Vì vậy trên máy bạn có một CA riêng, được
tạo lần đầu dùng, và khoá bí mật của nó không bao giờ rời khỏi máy.

```bash
mix cert ca-status
```

Lệnh này cho biết CA đó là gì: tên, fingerprint, còn hạn bao lâu. Máy bạn có *tin* nó hay không là
một câu hỏi khác, liên quan tới các trust store của hệ điều hành, và bản build này không trả lời ở
đây. Không có gì `ca-status` in ra ngụ ý câu trả lời cho câu hỏi đó.

Trên Linux có hai câu trả lời về độ tin cậy chứ không phải một, và MixLab giữ chúng tách biệt:
trust store của hệ thống, và cơ sở dữ liệu chứng chỉ riêng mà Chrome và Firefox đọc thay vì store
hệ thống. Một công cụ gộp hai cái làm một sẽ hiện dấu tích xanh ngay cạnh trình duyệt đang báo ổ
khoá đỏ.

## Chứng chỉ cho site

Chứng chỉ lá được cấp theo từng site, thời hạn 90 ngày, bao đúng các tên miền của site đó theo đúng
thứ tự của site.

```bash
mix cert issue --site blog.test
mix cert issue            # every HTTPS site
```

Việc cấp là **idempotent**: chứng chỉ nào vẫn bao đúng các tên, còn hơn ba mươi ngày, và được ký
bởi CA hiện tại thì được giữ nguyên. Nên chạy lệnh này không tốn gì, và là việc hợp lý khi bạn
không chắc.

## Chuyển hướng sang HTTPS

Khi site đã có chứng chỉ, cả `http://blog.test` lẫn `https://blog.test` đều chạy và phục vụ cùng
một site, vì mặc định không chuyển hướng. Đây là chủ ý: một webhook, một script cũ, hay bất cứ thứ
gì khác vẫn đang trỏ vào HTTP thuần vẫn tiếp tục hoạt động, và một request lúc bị chuyển hướng lúc
không là loại lỗi khó chịu hơn nhiều so với việc không bao giờ bị chuyển hướng.

Nếu có thứ gì đó truy cập site này từ bên ngoài mà cần chuyển hướng, bật riêng cho site đó:

```bash
mix site update blog.test --https-redirect true
```

Site phải bật HTTPS trước, vì MixLab từ chối bật chuyển hướng cho site chưa có gì để chuyển
hướng *đến*. Tắt HTTPS sau đó sẽ tự tắt luôn chuyển hướng, thay vì để nó bật cho một địa chỉ không
còn trả lời nữa.

Có một request không bao giờ bị chuyển hướng, dù cấu hình thế nào: điện thoại chưa cài certificate
authority của máy này vẫn phải truy cập được `/__mixengine/ca.crt` qua HTTP thuần, nên đường dẫn
đó luôn trả lời trực tiếp.

## Ổ khoá có thật sự xanh không?

```bash
mix cert status
```

Lệnh này không đọc từ đĩa. Nó mở một kết nối TLS thật tới front end của bạn cho từng site, rồi báo
lại chứng chỉ thật sự được đưa ra. Đó là thứ duy nhất trình duyệt nhìn thấy, và là cách duy nhất
để phát hiện server vẫn giữ một chứng chỉ đã bị thay bên dưới. Lệnh chỉ đọc: không cấp, không cài,
không reload gì cả.

## Thay CA

```bash
mix cert ca-rotate
```

**Có tính phá huỷ.** Mọi trình duyệt đang cache chuỗi chứng chỉ dưới CA cũ sẽ ngừng chấp nhận, và
chứng chỉ của mọi site được cấp lại. Không có gì bị thay nếu máy này không tin được CA mới: từ
chối hộp thoại xin quyền thì mọi thứ giữ nguyên như cũ.

Để máy ngừng tin CA của MixLab mà không gỡ gì khác:

```bash
mix cert ca-uninstall
```

Lệnh này gỡ CA khỏi mọi store đang tin nó, nhưng để nguyên file chứng chỉ trên đĩa và chứng chỉ
của mọi site. `mix doctor --repair` sẽ đưa lại độ tin cậy đó.
