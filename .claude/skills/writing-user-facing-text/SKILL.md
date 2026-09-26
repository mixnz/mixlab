---
name: writing-user-facing-text
description: Use when writing or changing text a MixLab user reads — an i18n string under apps/desktop/src (en or vi), a letter or error the sync server sends, a message `mix` or mixengined prints, any README.md in the repository or CHANGELOG.md — and when a string reads stiff, translated or machine-written.
---

# Text a person would have written

Write it the way a teammate would say it to the user sitting next to them: plain words, one idea
per sentence, what happened and what to do next.

## Shape by surface

| Surface | Shape |
| --- | --- |
| Button, menu, label | Verb + object, one to three words, no period: `Stop all`, `Lưu` |
| Error, notice | What happened, then what to do. Technical detail (`{{message}}`) goes last |
| Confirm dialog | Title names the action, body gives the consequence in one sentence, button repeats the verb |
| Email | The code, how long it works, what to do if it was not you |
| `mix`, mixengined | One line, lowercase, ending with the command that fixes it |
| `///` on a clap command or flag | Printed by `mix --help`. What it does for the user; task ids, history and `docs/` paths go in `//` |
| Any README | What this folder is for, then where to start: the first command, or the first file to read |
| CHANGELOG | What the user can do now, in their words. Not how it was built |

## Vietnamese: write it, don't translate it

Read the English once, close it, and write what a Vietnamese developer would say.

| Translated | Natural |
| --- | --- |
| trình cập nhật của MixEngine giữ nó luôn mới | MixEngine tự cập nhật MixLab |
| máy chủ đang đưa ra khóa khác | khoá của máy chủ đã thay đổi |
| mọi thứ được mã hoá trên máy này trước | MixLab mã hoá dữ liệu ngay trên máy này |
| Hãy ghi lại và cất ở nơi an toàn | Ghi lại và cất ở nơi an toàn |

- Spell `khoá`, `hoá`, the majority in this repo, and never mix spellings in one file.
- Keep the words developers say in English: tunnel, port, commit, SSH.

## Tells to rewrite

Each hit gets rewritten, not softened.

- Em dash (—) in any string above → a period, comma or colon.
- Contrast framing: "not X, but Y", "X — unless Y", "Hoặc… hoặc…".
- A closing aphorism restating the point: "Without the email it is not enough."
- Three items where only two are real.
- Filler: simply, just, seamlessly, robust, powerful, ensure, leverage; `một cách`, `việc`,
  `tiến hành`, `thực hiện`, `vui lòng`, `hãy` opening every sentence.
- Exclamation marks, emoji, "Please note", rhetorical questions, bold or Title Case inside a string.

## Before you finish

Read it aloud. If you would not say it to a colleague, rewrite it. Write the en and vi strings each
from scratch, then check they say the same thing.
