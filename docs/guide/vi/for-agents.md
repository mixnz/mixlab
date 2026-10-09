+++
title = "Đọc cẩm nang này bằng chương trình"
slug = "for-agents"
order = 16
summary = "Mọi trang của cẩm nang này đều là Markdown thuần tại một địa chỉ dễ đoán, kèm một manifest, một file gộp, và cùng nội dung đó nằm sẵn trong chương trình mix."
translation_of = "en/for-agents.md"
source_sha256 = "9805cdc7fede4a8bede2ea84f54b4872d3aca5968a6d13002e5051cd2a6eac94"
+++

# Đọc cẩm nang này bằng chương trình

> **Đây là tài liệu hướng dẫn dùng MixLab qua dòng lệnh `mix`.** Nếu bạn muốn thao tác bằng
> giao diện đồ hoạ cho dễ hơn thì bạn đã có sẵn: mọi bộ cài đều đặt sẵn cửa sổ MixLab ngay cạnh
> dòng lệnh. Cả hai đều điều khiển cùng một MixEngine bên dưới, nên mọi khái niệm trong cẩm nang
> này vẫn áp dụng.

Trang web này viết cho người đọc và phát hành cho chương trình đọc. Không có gì ở đây được render
bằng JavaScript, không trang nào là bản tóm tắt của một trang thật nằm ở chỗ khác, và mọi địa chỉ
bên dưới đều ổn định.

Nếu bạn là một agent đang giúp ai đó dùng MixLab: hãy đọc `llms.txt` trước, rồi lấy một hai
trang bạn cần ở dạng Markdown.

## Bắt đầu từ đây

```
https://mixnz.github.io/mixlab/llms.txt
```

Đây là mục lục mọi trang ở cả hai ngôn ngữ, mỗi trang có URL Markdown tuyệt đối và một câu tóm
tắt, cộng thêm các tài nguyên máy đọc được liệt kê bên dưới.

## Mọi địa chỉ

| Địa chỉ | Là gì |
| --- | --- |
| `/` | Trang chọn ngôn ngữ. Nội dung thật, không phải redirect |
| `/en/` và `/vi/` | Trang mục lục của mỗi ngôn ngữ |
| `/en/<slug>/` | Một trang, dạng HTML, cho người đọc |
| `/en/<slug>.md` | Cùng trang đó, dạng Markdown |
| `/en/llms-full.txt` | Mọi trang tiếng Anh nối lại, để gửi một request thay vì mười sáu |
| `/vi/llms-full.txt` | Tương tự, bằng tiếng Việt |
| `/llms.txt` | Mục lục nói ở trên |
| `/install.sh` | Script cài bằng một lệnh cho macOS và Linux |
| `/install.ps1` | Script tương tự cho Windows PowerShell |
| `/index.json` | Manifest nói ở dưới |
| `/sitemap.xml`, `/robots.txt` | Cho crawler |

**`/<locale>/<slug>.md` chính là file trong repo, từng byte một.** Không phải bản render lại, không
phải bản trích. Cùng những byte đó nằm trong `docs/guide/` của repo mã nguồn và được biên dịch vào
chương trình `mix`. Mỗi trang HTML cũng có thẻ `<link rel="alternate" type="text/markdown">` trỏ
tới bản Markdown của chính nó, nên chương trình nào lỡ vào trang HTML cũng không phải đoán.

Liên kết chéo giữa các trang được viết dạng `./<slug>.md`, và từ địa chỉ Markdown thì nó phân giải
đúng mà không cần viết lại gì.

## Manifest

```
https://mixnz.github.io/mixlab/index.json
```

```json
{
  "product": "MixLab",
  "version": "0.0.17",
  "base_url": "https://mixnz.github.io/mixlab/",
  "locales": ["en", "vi"],
  "pages": [
    {
      "locale": "en",
      "slug": "getting-started",
      "order": 3,
      "title": "Your first site",
      "summary": "From a fresh install to https://blog.test …",
      "html": "https://mixnz.github.io/mixlab/en/getting-started/",
      "markdown": "https://mixnz.github.io/mixlab/en/getting-started.md",
      "sha256": "…",
      "translation_of": null
    }
  ]
}
```

`sha256` được tính trên các byte của file Markdown, nên bạn kiểm tra được bản cache mà không cần
tải lại. `version` là bản phát hành MixLab mà site này mô tả.

## Offline, ngay trên máy

Mọi trang đều được biên dịch vào `mix`, và `mix docs` in ra đúng những byte đó mà không cần mạng,
không cần daemon đang chạy:

```bash
mix docs                       # list the topics
mix docs getting-started       # print one, as Markdown
mix docs getting-started --lang vi
mix docs getting-started --json
mix docs --reference           # the whole command reference
```

`--json` trả về `{ topic, locale, title, url, body }`, trong đó `body` đúng là thứ dạng thường in
ra. Đây là đường đáng tin khi không có mạng, và là đường đúng khi phiên bản trên máy mới là điều
quan trọng: các trang nằm trong một binary là phiên bản của binary đó, còn site này mô tả bản phát
hành hiện tại.

## Mọi lệnh đều trả lời bằng JSON

Không chỉ `docs`. `--json` là cờ toàn cục của `mix`:

```bash
mix status --json
mix site list --json
mix doctor --json
```

Lỗi cũng trả về dạng JSON, và cùng một cấu trúc dù daemon từ chối lời gọi hay `mix` không kết nối
được tới daemon nào: một `code` ổn định, một câu mô tả, và một `hint` khi có việc gì đó để làm. Hãy
rẽ nhánh theo `code`, đừng bao giờ theo câu mô tả.

## Nói chuyện trực tiếp với daemon

`mix` là một client mỏng bên trên API JSON-RPC cục bộ, qua Unix socket, hoặc named pipe trên
Windows. Toàn bộ hợp đồng được công bố dưới dạng kiểu TypeScript, sinh ra từ chính mã nguồn của
daemon và được CI kiểm tra đối chiếu với nó:

```
https://github.com/mixnz/mixlab/tree/master/bindings
```

Một file nén chứa các kiểu đó được đính kèm mỗi bản phát hành, ký bằng cùng khoá với các binary.
Những gì các kiểu này mô tả là những gì daemon **ghi ra**; vài request chấp nhận nhiều hơn những gì
được mô tả, và gửi đúng cấu trúc đã ghi trong tài liệu thì luôn được chấp nhận.

Phiên bản giao thức được biết qua bước handshake chứ không qua các kiểu, vì kết nối là đầu duy nhất
biết điều đó.

## Xử lý chuyện phiên bản

- Site mô tả một bản phát hành; `index.json` nói đó là bản nào.
- Daemon đang chạy tự báo phiên bản của nó qua `mix status --json`.
- Khi hai bên không khớp, daemon là sự thật về cái máy trước mặt bạn, còn site là sự thật về bản
  phát hành hiện tại.
