+++
title = "Máy chủ, cơ sở dữ liệu và bộ nhớ đệm"
slug = "services"
order = 6
summary = "Caddy hoặc Nginx, MariaDB, MySQL, PostgreSQL, Redis và Memcached. Cài khi bạn yêu cầu, cấu hình sẵn cho bạn, và không bao giờ in mật khẩu ra màn hình."
translation_of = "en/services.md"
source_sha256 = "0e1c5ac2325014ad8e4070f35b60f82b3714d7b977c2eb981da7d456bf4e20c2"
+++

# Máy chủ, cơ sở dữ liệu và bộ nhớ đệm

> **Đây là tài liệu hướng dẫn dùng MixLab qua dòng lệnh `mix`.** Nếu bạn muốn thao tác bằng
> giao diện đồ họa cho dễ hơn thì bạn đã có sẵn: mọi bộ cài đều đặt sẵn cửa sổ MixLab ngay cạnh
> dòng lệnh. Cả hai đều điều khiển cùng một MixEngine bên dưới, nên mọi khái niệm trong cẩm nang
> này vẫn áp dụng.

Có hai từ cần phân biệt, đúng như cách MixLab phân biệt chúng.

**Package** là một chương trình MixLab biết cách chạy, ví dụ Caddy, MariaDB, Redis. Cài package
chỉ là chép một bản của nó vào thư mục riêng của MixLab, không làm gì khác.

**Service** là một instance đang chạy của package: có cổng, thư mục dữ liệu, cấu hình sinh tự động,
log và trạng thái riêng. `mariadb@main` và `mariadb@legacy` là hai service của cùng một package,
với cổng khác nhau, dữ liệu khác nhau, và có thể cả phiên bản khác nhau.

## Có những gì

| Service | Dòng phiên bản mặc định | Cổng mặc định |
| --- | --- | --- |
| Caddy | 2.x | 80 và 443. Front end mặc định |
| Nginx | 1.27 | 80 và 443. Lựa chọn thay thế, mỗi lúc chỉ chạy một front end |
| php-fpm | một pool cho mỗi PHP đã cài | một socket, hoặc một cổng cục bộ trên Windows |
| MariaDB | 11.4 LTS | 3306 |
| MySQL | 8.4 LTS | 3306. Đây là sản phẩm khác MariaDB, không phải một phiên bản của nó |
| PostgreSQL | 16 | 5432 |
| Redis | 7.x | 6379 |
| Memcached | 1.6 | 11211 |
| MongoDB | 8.x | 27017. Không có tài khoản, nên chỉ lắng nghe trên loopback |

**Không có gì tự xuất hiện.** MixLab mới cài chưa có web server nào cho tới khi bạn cài. Chữ
"mặc định" ở bảng trên nghĩa là *thứ dự án này khuyên dùng khi có nhiều lựa chọn*, chứ không phải
*thứ đã có sẵn*.

## Cài và tạo

```bash
mix package available
mix package install mariadb 12.3.2
mix service create mariadb@main 12.3.2
```

Phần trước dấu `@` trong id service là tên package mà service đó là instance của. Vì thế
`mix service create` không cần thêm tham số riêng cho package. Phần sau dấu `@` là do bạn đặt, dùng
để phân biệt hai service với nhau; MixEngine không gán ý nghĩa gì cho chữ đó. Caddy chỉ chạy một
lần cho cả home MixEngine, nên service của nó chỉ đơn giản là `caddy`, không có `@`.

Tên service không cần ghi phiên bản. `mix service list` hiện phiên bản ngay cạnh id, MixLab cũng
vậy, nên đặt `mysql@main` là đủ.

Id không đổi được sau khi tạo, vì nó cũng là tên thư mục cấu hình sinh ra, thư mục log, socket, và
địa chỉ lưu mật khẩu. Muốn đổi tên thì tạo service mới rồi xóa cái cũ; dữ liệu vẫn được giữ lại.

Các cờ hữu ích của `mix service create`:

| Cờ | Tác dụng |
| --- | --- |
| `--port` | Cổng service lắng nghe. Bỏ trống thì dùng cổng mặc định của recipe |
| `--bind` | Địa chỉ service bind vào. Bỏ trống thì là `127.0.0.1` |
| `--data-dir` | Nơi lưu dữ liệu. Bỏ trống thì là một thư mục dưới home |
| `--autostart` | Tự khởi động mỗi khi daemon khởi động |

### Ai được cổng 3306

MariaDB và MySQL cùng muốn một cổng, và hai instance của cùng một loại cũng vậy. Quy tắc chỉ có
một: **ai tạo trước thì được trước**. Service đầu tiên xin 3306 sẽ được nó; service tiếp theo nhận
cổng trống đầu tiên phía trên. MixLab báo lại cổng nó đã chọn, vì cổng bạn không tự chọn thì bạn
cần được cho biết.

Nếu bạn ghi rõ cổng, MixLab dùng đúng cổng đó, không cấp phát gì thêm.

### Mỗi thư mục dữ liệu chỉ một service

`mix service create` từ chối `--data-dir` mà một service khác đang giữ, và nêu tên service đó. Hai
server cùng ghi lên một bộ file sẽ làm hỏng chúng, và cái giá đó rơi vào dữ liệu của bạn chứ không
phải vào một lần khởi động thất bại.

## Chạy service

```bash
mix service list
mix service status mariadb@main
mix service start mariadb@main
mix service stop mariadb@main
mix service logs mariadb@main --follow
```

`mix service status` bắt buộc có id, trong khi `start` và các lệnh còn lại thì id là tùy chọn. Hỏi
status mà không nói của cái gì thì thực ra là gõ nhầm `list`, và nếu trả lời bằng một danh sách thì
sẽ che mất lỗi đó.

Xóa service sẽ xóa bản ghi và cấu hình sinh ra từ nó, nhưng **không bao giờ xóa dữ liệu**, vì đó là
cơ sở dữ liệu của ai đó. Kết quả trả về nêu rõ thư mục còn để lại, để không ai phải đi tìm:

```bash
mix service delete mariadb@legacy
```

### Từ khay hệ thống

MixLab có một icon trên khay hệ thống (Windows, Linux) hoặc thanh menu (macOS), dù bạn dùng những
module nào. Khi bật MixEngine, trên Windows và macOS bấm vào icon sẽ trượt một bảng vào góc màn hình,
sát thanh taskbar hoặc menu bar. Trên Linux, bấm vào icon sẽ mở một menu ngắn, và **Mở bảng điều
khiển** mở cùng bảng đó dưới dạng một cửa sổ nhỏ, vì Linux không báo cho ứng dụng biết khi icon được
bấm. Không bật MixEngine thì không có bảng, bấm vào icon sẽ mở lại cửa sổ MixLab.

Đầu bảng có **Mở MixLab** và nút nguồn để thoát MixLab. Bên dưới, thẻ MixEngine hiện CPU và bộ nhớ,
cuối thẻ có **Tắt MixEngine**, nút này hỏi xác nhận trước. Tiếp theo là các service kèm nút Chạy và
Dừng, **Dừng tất cả**, và các site (bấm vào để mở).

Trên Windows và macOS, bấm chuột phải vào icon sẽ mở **Mở MixLab** và **Thoát MixLab**.

Bấm đóng cửa sổ MixLab chỉ ẩn cửa sổ đi, nên các phiên terminal và kết nối cơ sở dữ liệu vẫn mở.
Muốn thoát, dùng menu của icon, nút nguồn trong bảng, hoặc ⌘Q trên macOS. Thoát MixLab không tắt
MixEngine.

Muốn có icon ngay sau khi đăng nhập, bật **Mở MixLab trên khay hệ thống mỗi khi đăng nhập** trong
**Settings → Chung**. Công tắc này tách riêng với **Chạy MixEngine mỗi khi đăng nhập** (`mix
autostart`); bạn có thể bật một trong hai, hoặc cả hai.

Trên GNOME, icon cần extension *AppIndicator and KStatusNotifierItem Support*. Ubuntu có sẵn
extension này; Fedora và GNOME gốc thì không. Khi thiếu nó sẽ không có icon, đóng cửa sổ vẫn thoát
MixLab như trước, và công tắc đăng nhập sẽ mở cửa sổ thay cho icon.

## Web server nào đang phục vụ site của bạn

Tại một thời điểm, chỉ một trong hai Caddy và Nginx là front end: mọi site trong home đều đi qua nó,
và `mix service front-end` cho biết đó là cái nào.

```bash
mix service front-end
```

Đổi sang cái kia chỉ một lệnh, và đây là một thao tác thật chứ không phải một tùy chọn: server đang
dùng bị dừng, bản ghi của nó bị xóa, mọi site được render lại cho server mới, rồi server mới được
khởi động.

```bash
mix package install nginx 1.27.3
mix service set-front-end nginx
```

**Trong lúc đó không site nào truy cập được**, nên `mix` sẽ nói trước điều sắp xảy ra và hỏi bạn.
Thêm `--yes` khi chạy trong script.

**Trên Linux, server mới cần quyền để trả lời trên cổng 80 và 443**, và quyền đó thuộc về chính
chương trình chứ không thuộc về MixLab — nên đổi sang chương trình khác nghĩa là phải xin lại, và
một hộp thoại xin quyền có thể hiện ra. Nếu không ai cấp quyền thì **không có gì thay đổi cả**: bạn
vẫn ở trên server cũ, MixLab nói rõ điều đó, và `mix elevation grant` rồi chạy lại đúng lệnh trên
sẽ hoàn tất. macOS và Windows không cần xin quyền lần hai.

Có hai thứ không đi theo khi đổi, và MixLab nêu tên chúng thay vì lặng lẽ bỏ đi: các thiết lập
bạn đã ghi đè — một tùy chọn của `nginx.conf` chẳng có ý nghĩa gì với Caddy — cùng với giới hạn tài
nguyên hay chính sách idle bạn đã đặt cho server cũ. Thư mục dữ liệu của server cũ được giữ nguyên
tại chỗ.

## Cơ sở dữ liệu và tài khoản

Tạo cơ sở dữ liệu chỉ cần một lệnh, và lệnh này tự khởi động server nếu nó chưa chạy:

```bash
mix database create mariadb@main --name blog
mix database create mariadb@main --name shop --user shop_app
```

**Mặc định không in mật khẩu ra.** Mật khẩu được sinh ngẫu nhiên rồi lưu vào credential store của
hệ điều hành: Credential Manager trên Windows, Keychain trên macOS, Secret Service trên Linux. Thứ
được in ra là địa chỉ nơi nó được lưu, theo đúng tên và key của store. Nhờ vậy một client có thể
nói với bạn *"đã lưu trong credential store dưới tên …"* mà không ai phải hardcode quy tắc đặt tên
của MixLab.

Khi project cần chính mật khẩu đó, thường là để điền vào file `.env`, `mix database credentials` sẽ
in nó ra. Cờ `--password` trên lệnh `create` cho bạn tự chọn mật khẩu thay vì để MixLab sinh:

```bash
mix database credentials mariadb@main --user blog
mix database create mariadb@main --name shop --user shop-app --password
```

Không kèm giá trị thì `--password` sẽ hỏi và đọc một dòng từ standard input, nên dùng qua pipe cũng
được: `echo secret | mix database create … --password`. Chọn mật khẩu cho tài khoản đã tạo trước đó
sẽ thay đổi giá trị đang lưu, và server được đồng bộ lại theo đúng cách nó vẫn làm khi mật khẩu bị
lệch. Tuy nhiên, một tài khoản có sẵn trên server mà MixLab không giữ credential nào thì vẫn bị
từ chối, kể cả khi bạn đưa đúng mật khẩu. Biết mật khẩu không có nghĩa tài khoản đó là của bạn.

Để mở cơ sở dữ liệu bằng ứng dụng trên máy:

```bash
mix database client mariadb@main   # what is installed, and where this system looked
mix database open mariadb@main     # open it
```

`client` chỉ đọc: không khởi động gì, không mở gì. *"Chưa cài client nào"* là một câu trả lời chứ
không phải lỗi; nó cho biết MixLab đã tìm ở đâu và có thể tải client ở đâu.

`open` khởi động instance nếu nó đang dừng, đọc mật khẩu từ credential store **ngay lúc đó**, rồi
đưa cho client qua biến môi trường của chính tiến trình client. Mật khẩu không bao giờ được in ra,
không nằm trong tham số, nên cũng không bao giờ lọt vào lịch sử shell.

## Service được dùng bao nhiêu tài nguyên, và khi nào thì dừng

```bash
mix service limits mariadb@main
mix service limits mariadb@main set --memory 512 --cpu 50
mix service idle mariadb@main --after 30m
```

`limits` không kèm lệnh con thì đọc; `set` thay thế; `clear` xóa. **`set` thay thế tất cả các
trường, không chỉ trường bạn nêu.** Ví dụ `set --cpu 50` sẽ xóa luôn giới hạn bộ nhớ đang có. Vì
thế lệnh in ra cả ba trường của kết quả, để giới hạn bị xóa hiện ngay trên màn hình chứ không thành
bất ngờ về sau. Hệ điều hành thực sự áp đặt được gì thì mỗi hệ mỗi khác, và câu trả lời sẽ nói bạn
đang có loại nào trong hai loại: giới hạn **cứng** là một bức tường, chạm tới là service bị kill
hoặc lần cấp phát tiếp theo thất bại; giới hạn **cảnh báo** là một vạch được theo dõi, service có
thể vượt qua, khi đó MixLab cảnh báo và, nếu recipe cho phép, khởi động lại. Nếu vẽ một giới hạn
cảnh báo như thể nó là một bảo đảm thì đó là nói dối về dữ liệu của bạn.

`idle` cho biết khi nào service bị dừng vì không ai dùng, và hiện tại cái gì đang giữ nó mở.
**Không có gì bị dừng chỉ vì rảnh, trừ khi bạn yêu cầu**: site đang chạy thì cứ chạy. Muốn tiết kiệm
pin, bật *Tiết kiệm pin* trong Settings của MixLab, hoặc chạy `mix service save-resources --on`. Khi
đó pool PHP không ai dùng trong nửa tiếng, hoặc database/cache trong một tiếng, sẽ được tạm dừng, và
request tiếp theo cần tới sẽ bật lại — lần tải đầu có thể chậm một chút. Dù bật hay tắt,
`mix service idle` vẫn đặt được thời gian riêng cho một service, và `--after 0` nghĩa là không bao
giờ.

Bản thân web server không bao giờ bị dừng vì rảnh, và nó khởi động cùng MixEngine.

## Cấu hình sinh tự động

MixLab tự viết cấu hình cho mọi service nó chạy, từ những gì nó biết. Các file đó dùng xong bỏ:
chúng được sinh lại, không bao giờ được đọc ngược lại. Nên không có gì trong đó để bạn sửa, và
không có gì phải giữ cho đồng bộ. Nếu một thiết lập bạn cần chưa có cờ tương ứng, đó là thiếu sót
của MixLab, không phải lời mời bạn sửa file.
