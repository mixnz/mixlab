---
status: implemented
date: 2026-08-23
---

# Ngữ cảnh của tab, giữa các lần mở app

Ngày: 2026-08-23

## Mục tiêu

Hôm nay `shell/session.ts` nhớ đúng thanh tab: tab nào, module nào, tên gì, cái nào đang ở trước.
Mở app lên, tab về đủ, nhưng bên trong tab nào cũng là cửa trước của module — form kết nối trống,
sidebar REST không có request nào mở, terminal hỏi lại muốn vào đâu.

Spec này cho mỗi tab nhớ thêm **một tham chiếu tới thứ nó đang mở**, và mở lại thứ đó khi tab được
nhìn tới lần đầu.

Sau khi làm xong:

- Tab db đang nối tới một saved connection: mở lại app, bấm vào tab đó → nếu connection ấy còn
  trong danh sách thì tab tự nối lại, không phải bấm gì.
- Tab REST đang mở vài request: mở lại tab → đúng dãy request tabs ấy, đúng cái đang active. Không
  request nào bị gửi đi.
- Tab terminal đang chạy shell cục bộ: mở lại tab → shell ấy chạy lại. Đang ở một saved host: nếu
  host còn thì phiên SSH mở lại.
- Không có bí mật nào mới nằm trong `localStorage`. Cái được ghi chỉ là id.
- Session cũ (chưa có phần này) vẫn đọc được, không mất tab nào.

## Phi mục tiêu

- **Không khôi phục nội dung phiên.** Scrollback của terminal, kết quả query, response body — chết
  theo app như hiện nay. Cái được mở lại là *đích*, không phải *lịch sử*.
- **Không khôi phục thứ chưa được lưu.** Kết nối db gõ tay chưa Save, host SSH gõ tay: không có id
  để trỏ tới, và mật khẩu thì không được ghi ra. Mở lại là form trống. Đây là hệ quả trực tiếp của
  lằn ranh chỉ-lưu-id, không phải thiếu sót cần vá sau.
- **Không tự mở mọi tab lúc khởi động.** Lazy mount của `App.tsx` giữ nguyên: tab chỉ mount khi
  được nhìn tới lần đầu, nên bật app không mở sáu kết nối.
- **Không gửi lại request REST**, không chạy lại query.
- **Shell không hiểu nội dung state.** Không có `switch (moduleId)` nào ở `shell/`.
- **Không thêm jsdom hay test component.** Repo cố ý chỉ test logic thuần.

## Hiện trạng

Những gì đã có sẵn và spec này dựa vào:

| Chỗ | Dùng để làm gì |
| --- | --- |
| [`shell/session.ts`](../../apps/desktop/src/shell/session.ts) | `StoredTab`, `parseSession`, `readSession`, `writeSession` |
| [`shell/tabs.ts`](../../apps/desktop/src/shell/tabs.ts) | `TabInfo`, `retitleTab`, `rebadgeTab` — và luật bail-out theo identity |
| [`shell/module.ts`](../../apps/desktop/src/shell/module.ts) | `ModuleTabProps` |
| [`shell/App.tsx`](../../apps/desktop/src/shell/App.tsx) | `mounted`, `setTabBadges`, effect ghi session |
| [`db/DbTab.tsx:563`](../../apps/desktop/src/modules/db/DbTab.tsx#L563) | `openAndConnect(entry)` — áp saved connection vào form rồi nối |
| [`db/savedConnectionsStore.ts`](../../apps/desktop/src/modules/db/saved/savedConnectionsStore.ts) | `useSavedConnections()` |
| [`rest/RestTab.tsx:191`](../../apps/desktop/src/modules/rest/RestTab.tsx#L191) | `open(id)`, và `openIds`/`activeId` |
| [`rest/requestsStore.ts`](../../apps/desktop/src/modules/rest/requestsStore.ts) | `useRequestLists()`, `findRequest(lists, id)` |
| [`terminal/TerminalTab.tsx`](../../apps/desktop/src/modules/terminal/TerminalTab.tsx) | `start(choice)`, `dismiss()` |
| [`terminal/savedHostsStore.ts`](../../apps/desktop/src/modules/terminal/savedTargets.ts) | `useSavedHosts()` — [`savedHosts.ts`](../../apps/desktop/src/modules/terminal/savedTargets.ts) đã ghép secret từ keyring vào `config` |
| [`terminal/api.ts:13`](../../apps/desktop/src/modules/terminal/api.ts#L13) | `localShells()` → `LocalShell[]`, `name` là định danh bền |

Bốn điều kiểm được trong code, quyết định thiết kế bên dưới:

1. **Cả ba module đều đã có đúng một hàm để gọi lại.** `openAndConnect`, `open(id)`, `start(choice)`.
   Không module nào phải mở đường mới; phần thêm là biết gọi hàm nào với id nào.
2. **Bí mật đã nằm ngoài tầm với.** Mật khẩu db và secret của saved host ở trong kho thông tin đăng
   nhập của OS, và store ghép chúng vào trước khi trao ra. Nối lại từ một id không cần hỏi gì.
3. **Contract cố ý không có persistence API.** `ModuleDefinition` nói thẳng như vậy, và đoạn mở đầu
   `session.ts` hứa "Nothing about what was *inside* it". Đó là quyết định cũ đúng ở thời điểm nó
   được viết; spec này là nhu cầu đầu tiên phá nó, nên nó phải phá theo cách hẹp nhất có thể — xem
   mục 1.
4. **Không store nào nói được nó đã đọc xong chưa.** `savedConnectionsStore`, `savedHostsStore` và
   `requestsStore` đều để `loaded` là biến private, và snapshot ban đầu là danh sách rỗng. Một
   effect khôi phục nhìn vào đó sẽ kết luận nhầm "item đã bị xoá" trong khi thật ra file chưa đọc
   xong — xem mục 3. Đây là phần dễ bị bỏ sót nhất của spec.

## 1. Shell: một khe mờ đục

Shell nhận thêm một khe cho mỗi tab, ghi xuống `localStorage` cùng session, và **không bao giờ nhìn
vào bên trong**.

```ts
// shell/session.ts
export interface StoredTab {
  id: string;
  moduleId: string;
  title: string;
  /** Module tự đặt, module tự đọc. Shell chỉ mang qua. */
  state?: unknown;
}

// shell/tabs.ts
export interface TabInfo {
  id: string;
  moduleId: string;
  title: string;
  badges: TabBadge[];
  state?: unknown;
}

// shell/module.ts
export interface ModuleTabProps {
  active: boolean;
  onTitleChange: (title: string) => void;
  onBadgesChange: (badges: TabBadge[]) => void;
  /** Cái module này đã ghi lần trước, hoặc `undefined`. Đọc **một lần lúc mount** — xem bên dưới. */
  restored?: unknown;
  /** `undefined` nghĩa là "quên đi", không phải "không đổi". */
  onStateChange: (state: unknown) => void;
}
```

**`parseSession` không validate `state`.** Nó chỉ kiểm những gì nó tự vẽ được — id, title, moduleId
— rồi mang `state` qua nguyên vẹn. Lý do: chỉ module biết hình dạng của mình, và một shell biết
`savedId` nghĩa là gì là một shell biết module db tồn tại. Cái đã qua `JSON.parse` thì chắc chắn
serialise lại được, nên không có rủi ro nào ở tầng này. Một `state` rác chỉ làm module đó bỏ qua
việc khôi phục, không làm hỏng tab nào khác.

Session cũ không có `state` thì `state` là `undefined` — không cần migration.

**Reducer trong `tabs.ts`, bail-out theo identity**, đúng luật `rebadgeTab` đã ghi:

```ts
export function restateTab(tabs: TabInfo[], id: string, state: unknown): TabInfo[]
```

So bằng `Object.is`. Không có nó thì `onStateChange` gọi từ effect + closure mới mỗi render của
`App` = vòng lặp mà comment đầu `tabs.ts` đã cảnh báo bằng tên: "Maximum update depth exceeded".
Vì so theo identity, **module phải giữ object state ổn định** — dựng bằng `useMemo` như đang làm với
badges, hoặc chỉ gọi `onStateChange` trong event handler chứ không trong render.

**`restored` là ảnh chụp lúc mount, không phải prop sống.** Module đọc *giá trị* đúng một lần —
initializer của `useState` — rồi từ đó làm việc với bản đã chụp. Đọc reactive thì module tự ghi đè
chính nó ngay sau khi ghi. Shell không ép được điều này bằng kiểu, nên nó là một dòng doc comment
trên field và một dòng trong [adding-a-module](../standards/desktop/adding-a-module.md).

Đọc một lần không có nghĩa là *hành động* một lần ngay lúc mount: hai trong ba module còn phải chờ
store của mình đọc xong file (mục 3). Cái chạy một lần là việc chụp giá trị và việc thử khôi phục,
không phải cùng một khoảnh khắc.

**Tab chưa mount vẫn giữ state.** Nó nằm trong `TabInfo`, module chưa chạy nên không ai gọi
`onStateChange`, và effect ghi session trong `App.tsx` map thẳng từ `tabs`. Mở app, không bấm vào
tab đó, đóng app → ngữ cảnh còn nguyên. Đây là tính chất phải có test, vì nó là cách dễ nhất để
tính năng này tự xoá chính nó.

`App.tsx` thêm `setTabState(id, state)` — sinh đôi của `setTabBadges` — và truyền hai prop mới
xuống. Không có gì khác đổi.

## 2. Ba hình dạng, mỗi module một

```ts
// modules/db/tabState.ts
export interface DbTabState { savedId: string }

// modules/rest/tabState.ts
export interface RestTabState { openIds: string[]; activeId: string | null }

// modules/terminal/tabState.ts
export type TerminalTabState =
  | { kind: "ssh"; hostId: string }
  | { kind: "local"; shellName: string; cwd: string | null };
```

Mỗi file xuất một hàm thuần `parseXTabState(value: unknown): XTabState | null`, và **đó là chỗ
validation sống**. Cùng tinh thần với `parseSession`: cái đi vào là chuỗi một phiên bản cũ nào đó
của app đã ghi, nên không tin gì cả.

### db

Khôi phục: có `savedId`, danh sách đã đọc xong, tìm thấy entry → `openAndConnect(entry)`.

Ghi: trong `connect()` khi thành công. Nối từ saved → `onStateChange({ savedId })`; nối gõ tay →
`onStateChange(undefined)`. Trong `disconnect()` → `onStateChange(undefined)`.

**Saved id phải là tham số của `connect()`, không phải đọc `editingId`.** `openAndConnect` gọi
`applySavedConnection(entry)` rồi `connect(entry.config, entry.name)` ngay sau đó, mà
`setEditingId` bên trong `applySavedConnection` chưa kịp có hiệu lực — closure của `connect` vẫn
nhìn thấy giá trị cũ. Chữ ký thành `connect(overrideConfig?, title?, savedId?)`, và nhánh gõ tay
truyền `undefined` một cách có chủ ý. Không có nó thì tab đầu tiên nối một saved connection sẽ ghi
nhầm `null`, còn tab đã từng nối cái khác thì ghi nhầm id cũ.

Nhờ ghi ở cả hai nhánh của `connect()`, state không phụ thuộc vào việc `disconnect()` có chạy trước
hay không.

`connect_db` thất bại (server tắt, VPN chưa bật) đi đúng đường `catch` đang có: `ErrorBanner` trên
form. **State không bị xoá** — máy chủ tắt không có nghĩa là người dùng đã rời khỏi kết nối đó.

### rest

Khôi phục: lọc `openIds` qua `findRequest(lists, id)`, bỏ id không còn; `activeId` giữ nếu vẫn nằm
trong dãy đã lọc, không thì lấy phần tử cuối; dãy rỗng thì không làm gì.

Ghi: khi `openIds` hoặc `activeId` đổi. Dãy rỗng → `onStateChange(undefined)`.

Lưu ý một thứ đã có sẵn: `requests.ts` quét bỏ request rỗng ("husk") lúc load. Một request vừa mở
mà chưa gõ gì có thể không còn ở đó lần sau — bộ lọc phía trên xử lý đúng trường hợp này rồi.

### terminal

Khôi phục:

- `ssh`: chờ `useSavedHosts()` đọc xong, tìm `hostId` → `start({ kind: "ssh", config: host.config, hostId })`.
  Secret đã được `savedHosts.ts` ghép vào `config`.
- `local`: gọi `localShells()`, tìm theo `name` → `start({ kind: "local", shell, cwd })`. Không thấy
  (WSL distro đã gỡ, shell đã xoá) → về `TargetForm`.

Ghi: trong `start(next)` — `local` luôn ghi, `ssh` chỉ ghi khi `next.hostId !== null`. SSH gõ tay
ghi `undefined`.

Xoá: `dismiss()`. **Phiên chết (`exit`) mà chưa `dismiss` thì giữ** — tab vẫn đang "ở" chỗ đó, và
màn hình "phiên đã kết thúc" với nút Kết nối lại vẫn là màn hình của đích ấy.

`failed()` — mở không được — cũng giữ, cùng lý do với db: SSH hỏng không phải là rời đi.

## 3. Store phải nói được "đã đọc xong chưa"

Ba store cùng một hình dạng và cùng một thiếu sót: `loaded` là biến private, snapshot ban đầu rỗng.
Effect khôi phục không phân biệt được "chưa đọc xong" với "không còn item nào".

Mỗi store xuất thêm một cách hỏi. Hình dạng thống nhất cho cả ba, để không có ba câu trả lời khác
nhau cho cùng một câu hỏi:

```ts
export function useSavedConnectionsLoaded(): boolean
export function useSavedHostsLoaded(): boolean
export function useRequestListsLoaded(): boolean
```

Cùng cặp `subscribe`/`getSnapshot` đang có, chỉ khác cái nó trả về. `loaded` chỉ đi từ `false` sang
`true` một lần nên không có gì phải nghĩ về ổn định tham chiếu.

Kèm theo, mỗi tab giữ **một ref "đã thử khôi phục rồi"**. Store cập nhật (một tab khác lưu thêm
connection) sẽ đẩy snapshot mới xuống mọi tab; không có ref này thì effect chạy lại và nối lần hai.
Ref được đặt khi *đã thử*, dù thành công hay không.

## 4. Bảo mật

Cái được ghi vào `localStorage`, hết:

| Module | Ghi |
| --- | --- |
| db | một uuid của saved connection |
| rest | vài uuid của request |
| terminal | một uuid của saved host, **hoặc** tên shell (`powershell`, `wsl:Ubuntu`) và thư mục bắt đầu |

Không host, không cổng, không tên đăng nhập, không mật khẩu, không URL, không header, không token.
Một id trỏ tới thứ nằm ở chỗ đã được canh — `connections.json` cộng keyring, `terminal-hosts.json`
cộng keyring, `rest-requests.json`. Ai đọc được `localStorage` mà không đọc được những file kia thì
cũng chỉ biết là có một kết nối đã từng mở, không biết nó đi đâu.

Thư mục bắt đầu của shell cục bộ là đường dẫn trên máy người dùng — không phải bí mật, và nó là
thứ duy nhất không-phải-id trong bảng trên. Ghi ra đây để lần sau ai thêm field mới còn biết vạch
nằm ở đâu.

## 5. Kiểm thử

`npm test` — Vitest, chỉ logic thuần, không jsdom.

`shell/session.test.ts`:

- `state` đi qua `parseSession` nguyên vẹn, kể cả khi nó là object lồng nhau.
- Tab không có `state` vẫn parse được — session của bản cũ.
- `state` là rác (số, chuỗi, `null`) vẫn qua được và không làm hỏng tab nào khác. Shell không phải
  chỗ chặn nó.
- Tab bị bỏ vì module không còn thì mang `state` của nó đi theo.

`shell/tabs.test.ts`:

- `restateTab` trả về **đúng mảng cũ** khi state không đổi theo identity.
- `restateTab` với id không có trong danh sách trả về mảng cũ.

Ba file `tabState.test.ts`, mỗi module một:

- Hình dạng đúng → parse ra.
- `undefined`, `null`, sai kiểu từng field, mảng chứa phần tử không phải chuỗi → `null`.
- terminal: `kind` lạ → `null`; `cwd` vắng mặt hợp lệ.

Kiểm bằng tay, vì không có test component:

- Mở ba tab (db nối saved, REST hai request, terminal local), đóng app, mở lại: thanh tab đủ, chưa
  tab nào nối. Bấm từng tab → từng cái mở lại.
- Xoá saved connection ở tab khác rồi mới bấm vào tab db: form trống, không banner.
- Mở app rồi đóng ngay, không bấm tab nào: mở lại lần nữa vẫn còn ngữ cảnh.

## 6. Chia đợt

Mỗi đợt là một commit chạy được và `npm test` xanh.

1. **Khe ở shell.** `StoredTab.state`, `TabInfo.state`, `restateTab`, hai prop mới trong
   `ModuleTabProps`, `setTabState` trong `App.tsx`. Chưa module nào dùng. Test session và tabs.
2. **`loaded` cho ba store.** Ba hook, không ai gọi. Không có test — bốn dòng `useSyncExternalStore`
   giống hệt phần đã có.
3. **db.** `tabState.ts` + test, khôi phục và ghi trong `DbTab`.
4. **rest.** `tabState.ts` + test, khôi phục và ghi trong `RestTab`.
5. **terminal.** `tabState.ts` + test, khôi phục và ghi trong `TerminalTab`.
6. **Docs.** Đoạn mở đầu `session.ts` và comment `ModuleDefinition` đang hứa điều ngược lại; bảng
   contract trong [frontend.md](../architecture/desktop/frontend.md);
   [adding-a-module.md](../standards/desktop/adding-a-module.md) mục 2 liệt kê `ModuleTabProps`;
   CHANGELOG.

Đợt 1 và 2 độc lập nhau; 3, 4, 5 độc lập nhau nhưng đều cần 1 và 2.

## 7. Rủi ro

- **Vòng lặp render.** Rủi ro thật, và đã có tiền lệ: badges từng gây đúng nó. `restateTab` bail-out
  theo identity là hàng rào, nhưng hàng rào chỉ giữ được nếu module không dựng object state mới mỗi
  render. Luật: gọi `onStateChange` trong event handler, không trong render.
- **Nối lại hai lần.** Store phát snapshot mới khi tab khác lưu thứ gì đó. Ref "đã thử" là hàng rào;
  thiếu nó thì triệu chứng là hai connection tới cùng một máy chủ từ một tab.
- **Nối tới máy chủ mà người dùng không định nối.** Bấm vào tab db là mở kết nối thật — với
  read-only thì không sao, với production thì đó là một quyết định. Lazy mount làm nó thành hành
  động có chủ ý (phải bấm vào tab), chứ không phải hệ quả của việc mở app. Chấp nhận, và ghi ra đây
  vì đó là thay đổi hành vi lớn nhất của spec này.
- **Contract nới ra rồi khó thu lại.** `restored`/`onStateChange` là `unknown`, nên không có gì cản
  một module nhét cả một bản nháp query vào đó. Ranh giới là mục 4, và nó chỉ được canh bằng review.
