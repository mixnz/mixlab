---
status: implemented
date: 2026-08-18
---

# Tách the standalone client thành shell + module

Ngày: 2026-08-18

## Mục tiêu

Hôm nay "database" *là* toàn bộ app: `App.tsx` biết `DbKind`, `AppState` bên Rust chỉ biết
connection, và một tab luôn là một connection. Sắp tới app sẽ có thêm những phần khác — REST client,
terminal — nên cần một tầng **module** nằm trên database.

Sau refactor:

- Người dùng không nhận ra gì đã đổi. App chạy y hệt bản 0.0.12.
- Không một byte dữ liệu đã lưu nào đổi định dạng.
- Thêm một module là thêm một thư mục dưới `src/modules/` và `src-tauri/src/modules/`, cộng một
  dòng ở `src/shell/registry.ts` và một khối ở `src-tauri/src/modules/mod.rs`.
- Không còn file nào ngoài `modules/db/` biết khái niệm nào của database.

## Phi mục tiêu

Những thứ nằm ngoài lần này, ghi ra để không bị kéo vào:

- **Không viết REST client hay terminal.** Lần này chỉ dựng khung.
- **Không session restore** (mở lại app nhớ các tab đang mở). Hôm nay app không có, thêm vào là
  tính năng mới — và nó cần đúng cái lifecycle API mà mục "Contract" cố tình bỏ.
- **Không đổi tên command Tauri.** `mysql_query` vẫn là `mysql_query`, nên không file `api.ts` nào
  phải sửa chuỗi `invoke`.
- **Không đổi tên app hay service name keychain.** Cả hai giữ `<Old>`. Đổi service name sau khi đã
  có người dùng là mất toàn bộ password đã lưu.
- **Không tổng quát hoá persistence.** Mỗi module tự chọn file store của nó.

## Hiện trạng

| Chỗ | Số dòng | Vấn đề với mục tiêu |
| --- | --- | --- |
| `src/App.tsx` | 213 | `TabInfo` mang `kind: DbKind` và `readOnly` — database rò lên shell |
| `src/ConnectionTab.tsx` | 998 | Form kết nối + saved list + switch workspace theo kind |
| `src/App.css` | 1715 | Dòng 1–480 là tokens + chrome; từ `.login-view` trở đi phần lớn là db |
| `src/i18n/en.ts` | 1039 | ~30 trên ~34 nhóm top-level là của database |
| `src/components/` | 37 folder | `SqlTable` nằm cạnh `Button` |
| `src-tauri/src/commands.rs` | 1557 | Mọi command của mọi driver |
| `src-tauri/src/lib.rs` | 132 | Liệt kê ~90 command phẳng, `AppState` là state của app |

Verification hiện có: `npm run build` (`tsc` với `strict`, `noUnusedLocals`,
`noUnusedParameters` + `vite build`), `npm test` (vitest — `virtualRows`, `sql/guard`,
`sql/statements`, `mysql/columns`), `cargo check` trong `src-tauri`.

> [AGENT.md](../../apps/desktop/CLAUDE.md) hiện viết "There is no test suite". Câu đó đã sai — `npm test` tồn
> tại. Sửa nó là một phần của đợt 7.

## 1. Contract giữa shell và module

Đây là phần quyết định; mọi thứ khác là hệ quả.

`App.tsx` hôm nay biết hai khái niệm của database: `tab.kind` (vẽ logo engine) và `tab.readOnly`
(vẽ ổ khoá). Cả hai được tổng quát thành **badge** — module tự quyết tab của nó đeo gì, shell chỉ
vẽ.

```ts
// src/shell/module.ts — file duy nhất định nghĩa "module là gì"

/** Một dấu hiệu module muốn hiện trên tab của nó. Shell vẽ, không hiểu ý nghĩa. */
export interface TabBadge {
  id: string;
  icon: ReactNode;
  /** Đã dịch sẵn — shell không biết namespace i18n của module. */
  label: string;
  /** Lớp CSS module tự định nghĩa, ví dụ `kind-mysql`. */
  className?: string;
}

export interface ModuleTabProps {
  /** Tab này có đang ở trước mặt không. Mọi tab luôn mounted, nên phím tắt cần biết. */
  active: boolean;
  onTitleChange: (title: string) => void;
  onBadgesChange: (badges: TabBadge[]) => void;
}

export interface ModuleDefinition {
  id: string;
  /** Tên trong dropdown [+]. */
  labelKey: TranslationKey;
  Icon: ComponentType<{ size?: number }>;
  /** Tiêu đề tab lúc vừa mở, trước khi module tự đặt tên. */
  defaultTitleKey: TranslationKey;
  Tab: ComponentType<ModuleTabProps>;
}
```

```ts
// src/shell/registry.ts — nơi duy nhất liệt kê module
import { dbModule } from "../modules/db";

export const MODULES = [dbModule];
export const DEFAULT_MODULE_ID = "db"; // Ctrl+T mở cái này
```

`TabInfo` thành `{ id, moduleId, title, badges }`. Shell không còn `import type { DbKind }`.
`App.tsx` tra registry theo `moduleId` rồi render `<def.Tab active onTitleChange onBadgesChange />`.

Phía database: `DbTab` giữ nguyên logic, chỉ gộp hai callback `onReadOnlyChange` + `onKindChange`
thành một `onBadgesChange` trả mảng badge nó tự dựng (logo engine, cộng ổ khoá nếu read-only).

**Contract cố tình không có:** không lifecycle hook (`onClose`, `onSave`), không persistence API,
không event bus giữa các module. Database đã tự dọn bằng `useEffect` cleanup lúc unmount và tự lưu
qua `savedConnections` của nó. Thêm hook ở tầng shell bây giờ là phát minh nhu cầu chưa ai có.

**UI không đổi.** `[+]` chỉ mở dropdown khi `MODULES.length > 1`; với một module nó hoạt động y như
hôm nay (bấm là mở tab database). `Ctrl+T` luôn mở `DEFAULT_MODULE_ID`.

## 2. Cây thư mục frontend

```
src/
  main.tsx                     không đổi
  shell/
    App.tsx                    tab bar, dropdown [+], phím tắt, settings — không biết database
    App.css                    :root tokens + chrome (.app .tab-bar .brand .tab* .tab-content)
                               + lớp dùng chung (.context-menu .visually-hidden .select-*)
    glass.css
    module.ts                  ModuleDefinition, ModuleTabProps, TabBadge
    registry.ts                MODULES, DEFAULT_MODULE_ID
    theme.ts  update.ts        chỉ shell đọc
    components/                GlassFilter, SettingsModal, UpdateToast

  core/                        không biết gì về database, module nào cũng dùng được
    platform.ts  reload.ts  scroll.ts  clipboard.ts  textEntry.ts
    errors.ts  nativeContextMenu.ts  paneCache.ts  sidebarKeyboard.ts
    virtualRows.ts  virtualRows.test.ts

  components/                  CHỈ primitives
    Button Input Select ItemList Pagination ActionBar
    ConfirmDialog ContextMenu NameDialog ErrorBanner LoadingOverlay Tooltip JsonView
    dialogMotion.ts contextMenuPosition.ts

  icons/                       Icon.tsx, icons.tsx  (brands.tsx đi theo db)
  i18n/                        index.tsx, en.ts + vi.ts (phần dùng chung), dicts.ts

  modules/db/
    index.ts                   export dbModule: ModuleDefinition
    DbTab.tsx                  ConnectionTab.tsx hôm nay
    db.css                     .login-view .saved-list .saved-item .kind-* …
    types.ts filters.ts tools.ts transfer.ts
    savedConnections.ts savedConnectionsStore.ts
    queryHistory.ts queryDrafts.ts querySnippets.ts
    icons.tsx                  brands.tsx + DatabaseIcon
    i18n/en.ts  i18n/vi.ts
    sql/ mysql/ postgres/ mongo/ redis/          nguyên vẹn, chỉ đổi đường dẫn
    components/                CollationSelect ColumnDialog DatabaseActions DatabaseDialog
                               DatabaseStats Document DocumentNode DumpDialog FilterBar
                               IndexDialog InsertDocumentsDialog InsertRowsDialog NoSqlTable
                               QueryEditor RedisGroupKeys RedisKeyList RedisTypeBadge
                               RedisValue SqlEditor SqlTable TableDialog TableStructure
                               TransferOverlay
```

### Quy tắc phân loại

Một thứ vào `core/` hoặc `components/` khi **(a)** trong nó không có khái niệm database nào, **và**
**(b)** một module khác có lý do thật để dùng.

- `virtualRows` qua được: thuật toán ảo hoá danh sách thuần, có test riêng, không biết hàng của nó
  là gì.
- `JsonView` qua được: JSON viewer chỉ đọc, không dính BSON — REST client sẽ cần cho response body.
- `paneCache` qua được: `fileInto` là một helper LRU thuần trên `Map`, không biết nội dung entry.
- `FilterBar` không qua, dù tên nghe chung: nó dựng từ danh sách toán tử của SQL và Mongo.
- `QueryEditor` / `SqlEditor` không qua: dính chặt `sql/completion`, `sql/guard`, `sql/lint`.

**Một ca sát ranh:** `sidebarKeyboard` có cơ chế hoàn toàn chung (hộp tìm kiếm + `ItemList`, `↓`
trao bàn phím xuống list, `↑` ở hàng đầu trả về hộp) và phụ thuộc duy nhất vào `components/ItemList`
— nên nó vào `core/`. Nhưng tham số của nó đang tên `selectedDb`. Đổi tên nó thành thứ trung lập
(`selectedGroup`) là phần việc kèm theo lần di dời này; để nguyên thì `core/` mang một từ của
database trong chữ ký công khai.

## 3. Backend Rust

```
src-tauri/src/
  main.rs                  không đổi
  lib.rs                   ~35 dòng: plugin dùng chung, modules::db::register(builder),
                           .invoke_handler(modules::handler())
  error.rs                 dùng chung — AppError, macro err!
  secrets.rs               dùng chung — keychain, khoá theo id tuỳ ý nên module nào cũng cất được
  ssh/mod.rs               ssh_tunnel.rs hôm nay, nâng lên tầng chung
  modules/
    mod.rs                 pub mod db;  +  pub fn handler()
    db/
      mod.rs               pub fn register(builder) -> builder
      commands/            mod.rs (connect_db, disconnect_db, test_ssh_tunnel, các hàm tra handle)
                           mysql.rs postgres.rs mongo.rs redis.rs tools.rs
      models.rs            ConnectionConfig, DbKind, SshConfig
      state.rs             DbState { connections, running_queries }, DbHandle, ActiveConnection
      drivers/             mysql* postgres* mongo redis dump tools filters
```

**State thuộc về module.** `AppState` biến mất; không còn struct nào ở tầng app biết "connection"
là gì.

```rust
// modules/db/mod.rs
pub fn register<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.manage(state::DbState::default())
}
```

Tauri cho `.manage()` nhiều kiểu khác nhau, nên REST module sau này gọi `.manage(RestState::default())`
của riêng nó mà không đụng `DbState`.

**Danh sách command rời khỏi `lib.rs`.** Tauri chỉ nhận **một** `invoke_handler`, và
`generate_handler!` cần đường dẫn ở dạng chữ — không ghép được nhiều danh sách. Nên danh sách vẫn là
một, chỉ chuyển sang chỗ dành cho nó và chia khối theo module:

```rust
// modules/mod.rs
pub mod db;

/// Mọi command của mọi module. Một danh sách, vì Tauri chỉ nhận một handler —
/// nhưng chia khối theo module, và mỗi khối chỉ module đó được sửa.
pub fn handler<R: tauri::Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        // ── dùng chung ──
        crate::secrets::secrets_save,
        // ...
        // ── db ──
        db::commands::connect_db,
        db::commands::mysql_query,
        // ...
    ]
}
```

Chữ ký trả về phụ thuộc phiên bản Tauri 2 đang dùng và **phải kiểm chứng bằng `cargo check` ngay
đợt 1**. Nếu không chịu, phương án lùi là để `generate_handler!` nguyên trong `lib.rs`, vẫn chia
khối như trên — mất tính gọn của `lib.rs`, giữ toàn bộ phần còn lại.

**`ssh/` lên tầng chung** vì nó đã độc lập với database: nó chỉ mở một listener cục bộ và forward.
Terminal module gần như chắc chắn cần, và `known_hosts.json` là dữ liệu của app chứ không của riêng
database. Đường dẫn file không đổi.

**Chia `commands.rs`** (1557 dòng) thành `commands/` như cây trên — quyết định là làm, vì đây là
lần duy nhất phải mở file đó ra. Nó là phần duy nhất của design có thể bỏ mà không ảnh hưởng gì
khác: bỏ thì giữ một `modules/db/commands.rs` nguyên khối, mọi mục khác đứng nguyên.

## 4. Dữ liệu và i18n

### Không có migration

| Cái gì | Ở đâu | Sau refactor |
| --- | --- | --- |
| Saved connections | `connections.json`, key `saved` | y nguyên |
| Password / URI / SSH secret | keychain, service `<Old>`, 1 entry/id | y nguyên |
| Query history / drafts / snippets | `query-history.json`, `query-drafts.json`, `query-snippets.json` | y nguyên |
| Theme / accent / glass / language | `localStorage`: `<old>-theme`, `<old>-lang`, … | y nguyên |
| `known_hosts.json` | app data dir | y nguyên |

Cài bản mới đè bản cũ: mọi connection đã lưu vẫn ở đó. Đây là lý do tên file store và service name
keychain được giữ nguyên vẹn.

### Ghép từ điển

Các nhóm vẫn phẳng ở tầng trên cùng, nên **`t("connection.host")` không đổi ở bất kỳ call site
nào** — 1039 dòng bị chia đôi mà không một chỗ gọi `t()` phải sửa. `TranslationKey` vẫn suy ra tự
động, nên gõ sai key vẫn là lỗi biên dịch.

Nhóm `error:` là chỗ hai bên cùng đòi: shared cần `credentialStoreUnreachable` và toàn bộ nhóm SSH
(vì `ssh/` là tầng chung), db cần ~40 key về driver, ghi hàng, filter. Spread phẳng thì bên sau
**nuốt trọn** bên trước và mất im lặng ~10 key.

```ts
// src/i18n/dicts.ts
import { en as shared } from "./en";
import { dbEn } from "../modules/db/i18n/en";

export const EN = {
  ...shared,
  ...dbEn,
  /* Danh mục lỗi là thứ duy nhất nhiều module cùng góp vào — nó đối chiếu 1-1 với các key mà
     `err!` bên Rust phát ra, và Rust thì lỗi ở đâu cũng có. Ghép tay, một dòng, nên kiểu vẫn
     tự suy ra. */
  error: { ...shared.error, ...dbEn.error },
};

/* Ngoài `error`, hai từ điển không được đặt trùng nhóm nào — trùng là mất key mà không ai biết.
   Đây là lưới chặn: trùng thì không biên dịch được. */
type Collision = Exclude<Extract<keyof typeof shared, keyof typeof dbEn>, "error">;
const _noCollision: [Collision] extends [never] ? true : never = true;
```

`vi.ts` ghép y hệt, trong cùng file.

**Ràng buộc kèm theo:** file từ điển của module phải là dữ liệu thuần, không import gì từ `i18n/`.
`dicts.ts` import ngược từ `modules/` nên vòng chỉ khép lại một cách vô hại khi điều đó được giữ.
Quy tắc này phải vào `.agent/conventions/i18n.md` — phá nó thì hậu quả là `undefined` lúc chạy, chứ
không phải lỗi biên dịch.

### Luồng dữ liệu và lỗi: không đổi

`DbTab` → `<db>/api.ts` → `invoke("mysql_query", …)` → `modules::db::commands`. `errorMessage()` ở
`core/` vẫn dịch `AppError` bằng `t()`. Không thêm tầng, không thêm context provider ở shell. Shell
không thấy lỗi của module — module tự dựng `ErrorBanner` của nó.

## 5. Chia đợt

Bảy đợt, mỗi đợt build xanh và commit được riêng.

| # | Đợt | Kiểm chứng | Commit |
| --- | --- | --- | --- |
| 1 | Backend: `modules/db/`, `ssh/`, `DbState`, `modules::handler()`, chia `commands/` | `cargo check`, `dev:app` kết nối được | `refactor(backend): move database code under modules/db` |
| 2 | Shell contract: `shell/`, `module.ts`, `registry.ts`, `ConnectionTab` → `modules/db/DbTab.tsx` + badges | `npm run build`, `npm test`, smoke | `refactor(shell): make the tab bar module-agnostic` |
| 3 | Gom file db: `sql/ mysql/ postgres/ mongo/ redis/`, savedConnections, query*, tools, transfer, filters, types, brands | `npm run build`, `npm test`, smoke | `refactor(db): gather the database module` |
| 4 | Tách `core/` và `components/` (23 folder sang `modules/db/components/`) | `npm run build`, `npm test` | `refactor(components): keep only shared primitives` |
| 5 | Tách `App.css` → `shell/App.css` + `modules/db/db.css` | smoke, xem kỹ bằng mắt | `refactor(styles): split App.css by owner` |
| 6 | Tách i18n + `dicts.ts` + guard | `npm run build` | `refactor(i18n): let each module own its strings` |
| 7 | Tài liệu | đọc lại | `docs(agent): describe the module layout` |

Đợt 1 đi đầu có chủ ý: nó là đợt duy nhất có rủi ro kỹ thuật thật (chữ ký `modules::handler()`), và
nó không đụng frontend, nên nếu phải lùi thì lùi rẻ.

**Smoke test thủ công** sau đợt 2, 3, 5: `npm run dev:app` → kết nối MySQL test server → mở một
bảng → chạy một câu query → mở tab thứ hai sang PostgreSQL → đóng tab → kiểm saved connection còn
nguyên. Đây là phần `tsc` không nói được gì: nó xác nhận đường dẫn đúng chứ không xác nhận app còn
chạy.

**Đợt 7 sửa:** [AGENT.md](../../apps/desktop/CLAUDE.md) (mục Layout, và câu "There is no test suite"),
[.agent/architecture/overview.md](../architecture/desktop/overview.md),
[.agent/architecture/frontend.md](../architecture/desktop/frontend.md),
[.agent/architecture/backend.md](../architecture/desktop/backend.md),
[.agent/conventions/adding-a-command.md](../standards/desktop/adding-a-command.md),
[.agent/conventions/i18n.md](../standards/desktop/i18n.md), và thêm
`.agent/conventions/adding-a-module.md`.

## 6. Rủi ro

1. **Chữ ký `modules::handler()`.** Kiểu trả về của `tauri::generate_handler!` phụ thuộc phiên bản.
   Kiểm ở đợt 1; phương án lùi là giữ `generate_handler!` trong `lib.rs`.
2. **Thứ tự CSS.** `App.css` và `glass.css` đang được import theo thứ tự có chủ ý trong `App.tsx`
   (glass phải sau). Thêm `db.css` import từ `modules/db` thì Vite quyết thứ tự theo đồ thị import.
   Lỗi sẽ hiện ra ở đợt 5 dưới dạng giao diện lệch, không phải lỗi biên dịch. Xử lý: nhìn bằng mắt
   sau đợt 5; nếu lệch thì `shell/App.css` `@import` `db.css` để thứ tự là tường minh.
3. **Import chéo còn sót.** Một component trong `components/` vẫn import từ `modules/db/` là
   boundary đã thủng, và `tsc` biên dịch bình thường. Cuối đợt 4 chạy
   `grep -rn "modules/db" src/components src/core src/shell/` — kết quả phải rỗng trừ
   `shell/registry.ts`.
4. **Vòng import i18n.** Xem ràng buộc ở mục 4.

## 7. CHANGELOG

**Không có entry.** Mục tiêu là người dùng không nhận ra gì đã đổi, nên theo
[.agent/conventions/changelog.md](../standards/desktop/changelog.md) thì không có dòng nào.
Nếu có, tức là đã làm sai gì đó.
