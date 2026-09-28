import type { ServiceState, StoppedBy } from "@mixengine/api";

/**
 * Khoá dịch cho một `ServiceState`, hoặc `null` nếu không có khoá nào.
 *
 * **Một bảng cho mọi màn hình.** Dashboard và Extensions cùng vẽ trạng thái service, và trước đây
 * cả hai in thẳng chuỗi trên wire — nên `running` hiện ra y nguyên bằng tiếng Anh ở một app đã
 * dịch phần còn lại. Hai chỗ tự dịch lấy sẽ thành hai cách gọi một trạng thái.
 *
 * **`null` chứ không phải một khoá đoán bừa.** `ServiceState` là enum đóng — doc của nó nói thẳng
 * "a state machine with room for one more state is one nobody can reason about" — nhưng cái đọc ở
 * đây là chuỗi một daemon gửi tới, và daemon có thể mới hơn bản MixDB đang chạy. Một trạng thái lạ
 * phải hiện ra đúng như daemon viết, không được biến thành ô trống hay thành một khoá dịch không
 * tồn tại.
 */
export type ServiceStateKey =
  | `mixengine.serviceState.${ServiceState}`
  | "mixengine.serviceState.resting";

/**
 * `stoppedBy` là thứ tách **"đang nghỉ"** khỏi "đã dừng" — T167g, ADR 0041. Một service MixEngine tự
 * dừng vì rảnh (`stopped` + `daemon`) sẽ được bật lại ở request kế tiếp, nên nó không phải hỏng và
 * cũng không phải thứ người dùng đã tắt: vẽ nó là `Stopped` đỏ là nói dối về thứ người dùng sẽ thấy.
 * Daemon cũ hơn không gửi `stopped_by`, và khi đó mọi thứ hiện y như trước.
 */
export function serviceStateKey(
  state: string | null | undefined,
  stoppedBy?: StoppedBy | null,
): ServiceStateKey | null {
  if (state === "stopped" && stoppedBy === "daemon") return "mixengine.serviceState.resting";
  return state != null && isServiceState(state) ? `mixengine.serviceState.${state}` : null;
}

/**
 * Câu giải thích đi kèm nhãn trạng thái, cho tooltip, hoặc `null` khi nhãn đã tự nói đủ.
 *
 * Nhãn nằm trong pill và trong những ô hẹp — bảng Dashboard, tray — nên phải ngắn; "sẽ tự bật lại"
 * là phần người dùng cần biết nhưng không cần đọc mỗi lần liếc qua.
 */
export function serviceStateHint(
  state: string | null | undefined,
  stoppedBy?: StoppedBy | null,
): "mixengine.serviceState.restingHint" | null {
  return state === "stopped" && stoppedBy === "daemon" ? "mixengine.serviceState.restingHint" : null;
}

/**
 * Ba sắc thái một trạng thái được vẽ bằng, hoặc `null` khi không biết trạng thái đó là gì.
 *
 * Ba chứ không phải bảy: màu ở đây trả lời "có đang phục vụ không", không phải "đang ở state nào"
 * — chữ đã nói state rồi. `degraded` là `busy` chứ không phải `bad`: nó vẫn trả lời, chỉ là không
 * khoẻ, và đỏ dành cho thứ không trả lời.
 *
 * Đi cùng `serviceStateKey` trong một file vì hai hàm phải bao đúng một tập tên; test bắt điều đó.
 */
export type ServiceTone = "ok" | "bad" | "busy" | "resting";

/** `resting` là sắc thái thứ tư, xám: không phục vụ lúc này, nhưng cũng chẳng có gì hỏng. */
export function serviceStateTone(
  state: string | null | undefined,
  stoppedBy?: StoppedBy | null,
): ServiceTone | null {
  if (state === "stopped" && stoppedBy === "daemon") return "resting";
  return state != null && isServiceState(state) ? TONE[state] : null;
}

const TONE: Record<ServiceState, ServiceTone> = {
  running: "ok",
  stopped: "bad",
  failed: "bad",
  starting: "busy",
  stopping: "busy",
  restarting: "busy",
  degraded: "busy",
};

/**
 * Nút công tắc của một hàng đang ở chế độ nào.
 *
 * Ba chế độ, và chúng trả lời **"bấm vào thì được gì"**, không phải "service đang ở state nào":
 *
 * - `up` — nó đang trả lời, nên việc còn lại là tắt. `degraded` nằm ở đây: nó chạy yếu chứ không
 *   phải không chạy, và tắt vẫn là việc duy nhất có nghĩa.
 * - `down` — nó không trả lời, nên việc là bật. Cả một state lạ cũng vào đây: mời bật một thứ đã
 *   chạy thì daemon từ chối một câu đọc được, còn không mời gì cả thì hàng đó thành ngõ cụt.
 * - `moving` — đang chuyển, không có việc nào để bấm.
 *
 * `inFlight` là "vừa gửi một hành động và chưa có sự kiện nào xác nhận". Lúc đó `state` vẫn còn
 * giá trị cũ, nên nếu chỉ nhìn `state` thì nút sẽ mời bấm lần nữa đúng cái vừa bấm.
 */
export type ToggleMode = "up" | "down" | "moving";

export function toggleMode(state: string | null | undefined, inFlight: boolean): ToggleMode {
  if (inFlight) return "moving";
  if (state === "starting" || state === "stopping" || state === "restarting") return "moving";
  return state === "running" || state === "degraded" ? "up" : "down";
}

/** Thu hẹp một chuỗi trên wire về enum — chỗ duy nhất biết bảy tên đó, và không có `as` nào. */
function isServiceState(value: string): value is ServiceState {
  return (KNOWN as ReadonlySet<string>).has(value);
}

const KNOWN: ReadonlySet<ServiceState> = new Set<ServiceState>([
  "stopped",
  "starting",
  "running",
  "degraded",
  "stopping",
  "restarting",
  "failed",
]);
