import type { SiteOwner } from "@mixengine/api";
import type { SiteSummary } from "@mixengine/api";

import { joinPath, nativePath, PATH_STYLE, type PathStyle } from "../../core/paths";

export type SiteRow = SiteSummary;

/**
 * Chỉ site thuộc một project mới sửa được.
 *
 * Site của một extension chỉ xem/liệt kê được ở đây — `site.update` gửi thẳng vào nó vẫn bị daemon
 * từ chối dù UI có cho phép, nhưng để nút bấm luôn hỏng là hứa một hành động không giữ được.
 */
export function canEditSite(owner: SiteOwner): boolean {
  return owner.type === "project";
}

/**
 * Áp `site_sharing_changed` lên bảng site.
 *
 * Cùng luật Dashboard đã theo cho `service_state_changed`: sự kiện là best-effort, nhưng khi tới nó
 * là nguồn thật, không phải suy đoán. `type` lạ hoặc payload hỏng bị bỏ qua, không ném — một biến
 * thể sinh ra ở phiên bản sau phải tới được một MixDB cũ như một object bỏ qua được.
 */
export function applySharingChange(rows: SiteRow[], raw: string): SiteRow[] {
  let event: unknown;
  try {
    event = JSON.parse(raw);
  } catch {
    return rows;
  }
  if (
    typeof event !== "object" ||
    event === null ||
    (event as { type?: unknown }).type !== "site_sharing_changed"
  ) {
    return rows;
  }
  const { domain, sharing } = event as { domain: string; sharing: SiteRow["sharing"] };
  return rows.map((row) => (row.domain === domain ? { ...row, sharing } : row));
}

/** Tên hiển thị mỗi domain gõ vào, tách bằng dấu phẩy hoặc xuống dòng — đầu danh sách là chính.
 *  Dùng chung giữa `SiteForm` và khối "tạo nhanh site" trong `ProjectForm`. */
export function parseDomains(raw: string): string[] {
  return raw
    .split(/[,\n]/)
    .map((d) => d.trim())
    .filter((d) => d !== "");
}

/**
 * Phần còn lại của một đường dẫn tuyệt đối sau khi bỏ project root — dùng ngay sau khi dialog chọn
 * thư mục (luôn trả tuyệt đối) trả về, để field Doc root chỉ giữ đúng phần daemon thật sự lưu
 * (`SiteSummary.doc_root`), viết theo dấu phân cách của hệ điều hành như daemon gửi về (T191), để
 * field giữ cùng một chuỗi dù đến từ Browse hay từ daemon. Không nằm dưới root thì giữ nguyên tuyệt
 * đối — `SiteCreate.doc_root` chấp nhận cả hai, đây là trường hợp hiếm không đáng chặn.
 */
export function relativeToRoot(
  root: string,
  absolute: string,
  style: PathStyle = PATH_STYLE,
): string {
  const normalizedRoot = root.replace(/[\\/]+$/, "");
  if (absolute === normalizedRoot) return "";
  for (const separator of ["/", "\\"]) {
    const prefix = `${normalizedRoot}${separator}`;
    if (absolute.startsWith(prefix)) return nativePath(absolute.slice(prefix.length), style);
  }
  return absolute;
}

/** Nối root với phần còn lại để hiển thị, theo dấu phân cách của hệ điều hành (T191) — chỉ để đọc,
 *  không phải giá trị gửi lên daemon (đó vẫn là phần còn lại một mình). `""` là chính root, đúng
 *  nghĩa `SiteSummary.doc_root` ghi. */
export function joinDocRoot(root: string, relative: string, style: PathStyle = PATH_STYLE): string {
  if (relative === "") return root;
  return joinPath(root, relative, style);
}

/** `mm:ss`, hay `hh:mm:ss` một khi còn hơn một giờ. Quá hạn kẹp về 0, không âm. */
export function formatRemaining(untilMs: number, nowMs: number = Date.now()): string {
  const totalSeconds = Math.max(0, Math.round((untilMs - nowMs) / 1000));
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return hours > 0 ? `${pad(hours)}:${pad(minutes)}:${pad(seconds)}` : `${pad(minutes)}:${pad(seconds)}`;
}

/**
 * Địa chỉ mở được của một site — T117.
 *
 * `SiteSummary` mang domain và một cờ `https`, không mang URL: daemon trả *site là gì*, còn ghép
 * thành một địa chỉ là việc hiển thị. Ở đúng một chỗ vì hai chỗ sẽ lệch nhau đúng vào ngày một
 * trong hai được sửa.
 *
 * **`https` là *khai báo*, không phải chứng chỉ đã cấp xong.** Một site vừa tạo có `https: true`
 * trước khi ai kịp cho phép cài CA; link này vẫn là link đúng để mở, còn trình duyệt cảnh báo gì
 * thì là câu chuyện của lượt elevation chưa chi.
 */
export function siteUrl(site: { domain: string; https: boolean }): string {
  return `${site.https ? "https" : "http"}://${site.domain}`;
}

/** Hai việc một cú bấm vào domain sinh ra, tách khỏi việc *làm* chúng. */
export interface SiteVisit {
  /** Project cần bật service trước khi mở, hoặc `null` nếu không có gì để bật. */
  startProject: string | null;
  /** Địa chỉ mở ra sau đó. */
  url: string;
}

/**
 * Bấm vào domain của một site thì phải làm gì.
 *
 * **Site của extension chỉ mở, không bật gì** — `service.start` nhận một *tên project*, mà một site
 * extension không có project nào; đoán bừa một cái tên là gửi cho daemon một thứ nó sẽ từ chối.
 * Thứ phục vụ nó là việc của extension ấy, và nút vẫn bấm được thay vì thành một hàng chết giữa
 * bảng.
 *
 * Trạng thái `disabled` cố ý *không* xét ở đây: nó nói web server có sinh server block hay không,
 * và mở ra để thấy đúng lỗi ấy vẫn là câu trả lời, không phải một nút bấm không ăn.
 */
export function siteVisit(site: {
  domain: string;
  https: boolean;
  owner: SiteOwner;
}): SiteVisit {
  return {
    startProject: site.owner.type === "project" ? site.owner.name : null,
    url: siteUrl(site),
  };
}
