+++
title = "Blueprint"
slug = "blueprints"
order = 9
summary = "Ghi lại một dự án gồm những gì, rồi dựng lại y hệt ở nơi khác, hoặc trên máy của người khác."
translation_of = "en/blueprints.md"
source_sha256 = "5f5f1360eb4746975ca84d85884dfe890de010a7d99e4c8f1dd2a9565c24ed26"
+++

# Blueprint

> **Đây là tài liệu hướng dẫn dùng MixLab qua dòng lệnh `mix`.** Nếu bạn muốn thao tác bằng
> giao diện đồ hoạ cho dễ hơn thì bạn đã có sẵn: mọi bộ cài đều đặt sẵn cửa sổ MixLab ngay cạnh
> dòng lệnh. Cả hai đều điều khiển cùng một MixEngine bên dưới, nên mọi khái niệm trong cẩm nang
> này vẫn áp dụng.

Blueprint là bản ghi mô tả một project gồm những gì: cần PHP nào, dùng service nào, các site trông
ra sao, và tuỳ chọn thêm một lệnh để scaffold ra một bản mới. Đây là cách bạn dựng cùng một môi
trường hai lần: trên máy thứ hai, cho đồng nghiệp, hoặc cho project tiếp theo có cùng cấu trúc.

## Ghi lại một blueprint

```bash
cd ~/code/blog
mix blueprint capture blog-stack --description "PHP 8.3, MariaDB, Redis"
mix blueprint list
```

Tên là thứ blueprint được lưu dưới đó, gồm chữ thường, chữ số và dấu gạch ngang.

**Blueprint ghi lại hình dạng, không ghi nội dung.** Nó ghi rằng project dùng MariaDB và phiên bản
nào; nó không ghi dữ liệu của bạn, và không bao giờ chứa mật khẩu. Áp dụng blueprint cho bạn cùng
một môi trường, chứ không phải bản sao công việc của bạn.

## Áp dụng một blueprint

```bash
mix blueprint apply blog-stack --project shop --dry-run
mix blueprint apply blog-stack --project shop
```

**Hãy chạy dry run trước.** Nó in ra kế hoạch và không thay đổi gì: runtime nào sẽ được cài,
service nào sẽ được tạo, site sẽ tên gì, và nếu có thì lệnh scaffold nào sẽ được chạy. Không có
bước nào của việc áp dụng bị giấu khỏi kế hoạch này.

`--path` chỉ định project mới nằm ở đâu. Mặc định là một thư mục đặt theo tên project, nằm dưới
thư mục hiện tại.

### Trả lời các câu hỏi về phiên bản

Blueprint đòi PHP 8.3 trên máy chỉ có 8.2 là một câu hỏi, không phải lỗi. Hai cờ sau trả lời trước
cho mọi câu hỏi kiểu đó trong kế hoạch:

| Cờ | Nghĩa |
| --- | --- |
| `--install-missing` | Cài đúng thứ blueprint yêu cầu |
| `--use-installed` | Dùng thứ máy này đã có sẵn |

## Import blueprint của người khác

```bash
mix blueprint import ./blog-stack.toml
```

Blueprint từ nơi khác có thể kèm chữ ký rời: `mix` tìm file `<file>.minisig` nằm cạnh, hoặc nhận
qua `--signature`. Và đây là luật quan trọng:

**Cái gì đến mà không có chữ ký được gallery xác nhận thì mãi mãi là không tin cậy.** Không có gì
nâng trạng thái đó lên về sau. Import lại kèm chữ ký cũng không rửa được nó; trạng thái tin cậy
được quyết định một lần, lúc import, và mọi danh sách có nêu tên blueprint đó đều hiển thị nó.

Trạng thái này không phải để trang trí. Nó quyết định lệnh `[scaffold]` của blueprint phải được
bạn đồng ý rõ ràng tới mức nào trước khi chạy.

## Lệnh scaffold, và vì sao phải hỏi

Blueprint có thể mang theo một lệnh chạy một lần trong project mới, ví dụ
`composer create-project …`, hoặc lệnh tương đương của framework mà nó dành cho. Đó là chương trình
của người khác chạy trên máy bạn, nên MixLab in ra chính xác lệnh đó và hỏi trước khi chạy. Cách
hỏi khác nhau tuỳ blueprint đến từ đâu.

Hai cờ bỏ qua câu hỏi này, và **cờ nào chỉ dùng cho trường hợp của cờ đó**:

| Cờ | Dành cho |
| --- | --- |
| `--run-scaffold` | Blueprint được gallery ký |
| `--run-untrusted-scaffold` | Blueprint không tin cậy. Không ai bảo đảm cho thứ lệnh này chạy |

Script nào chạy lệnh chưa ký của người khác thì nên ghi rõ điều đó ngay trên dòng chạy lệnh. Vì
thế mới có hai cờ thay vì một, và trong cả hai trường hợp lệnh đều được in ra trước
khi chạy.

## Theo dõi quá trình áp dụng

Áp dụng blueprint là một job. Nó có thể cài runtime, tạo service và chạy scaffold, nên có thể mất
một lúc:

```bash
mix job list
mix job status <id>
mix job logs <id>
mix job wait <id>
```

`mix job logs` là nơi output của lệnh scaffold hiện ra; đó là thứ duy nhất trong quá trình áp dụng
tự in ra gì đó. Các dòng log tồn tại chừng nào daemon còn giữ job, nên đây là thứ để đọc trong lúc
job chạy, không phải bản ghi để tuần sau quay lại xem.

Nếu việc áp dụng cần quyền quản trị, ví dụ một tên miền mới cần định tuyến, nó hỏi một lần ở cuối.
`--grant` dùng luôn hộp thoại đó mà không hỏi trước.
