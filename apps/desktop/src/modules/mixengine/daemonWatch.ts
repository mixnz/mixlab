import * as api from "./api";

/**
 * Kênh sự kiện MixEngine dùng chung cho cả app.
 *
 * `api.watch()`/`api.unwatch()` là một cặp invoke toàn cục phía daemon — không phải một kênh riêng
 * cho từng màn hình. Trước đây mỗi màn hình (Dashboard, Sites, Packages/Languages, Packages/PackageList)
 * tự gọi `watch()` lúc mount và `unwatch()` lúc unmount, dựa hẳn vào việc luôn chỉ một trong số đó
 * được mount cùng lúc. Giữ nhiều màn hình mount cùng lúc (để không mất tiến độ job khi chuyển tab) phá
 * vỡ giả định đó: màn hình mount sau sẽ giành mất kênh của màn hình mount trước.
 *
 * Cách của `db/tools.ts` với `tools://progress`: mở kênh đúng một lần cho cả đời app, không bao giờ
 * đóng lại — mỗi bên chỉ thêm/bớt callback của mình khỏi một tập hợp cục bộ, không đụng gì tới daemon.
 */
/** What `src-tauri/src/modules/mixengine/events.rs` sends when the stream ends. */
const DISCONNECTED = '{"type":"mixdb_disconnected"}';

const listeners = new Set<(raw: string) => void>();
let watching: Promise<void> | null = null;

function ensureWatching(): void {
  watching ??= api.watch((raw) => {
    // A stream that ended is not open any more: the next `ensureDaemonWatch` reopens it (T168 —
    // the tray panel outlives many daemons, and without this it would never hear from the next).
    if (raw === DISCONNECTED) watching = null;
    for (const listener of listeners) listener(raw);
  }).catch(() => {
    // Cho phép lần subscribe sau thử lại — một watch hỏng lúc mở kênh không nên khoá vĩnh viễn.
    // Không có màn hình cụ thể nào để báo lỗi này nữa (kênh dùng chung), nên chỉ nuốt và thử lại.
    watching = null;
  });
}

/** Đăng ký nhận mọi message thô từ kênh MixEngine. Gọi hàm trả về để huỷ đăng ký — việc đó không
 *  đóng kênh, chỉ gỡ callback này khỏi danh sách nhận. */
export function subscribeDaemonWatch(listener: (raw: string) => void): () => void {
  listeners.add(listener);
  ensureWatching();
  return () => {
    listeners.delete(listener);
  };
}

/** Opens the channel again if it is not open — after the daemon it was open to has gone and another
 *  has started. Harmless when it is open. */
export function ensureDaemonWatch(): void {
  ensureWatching();
}
