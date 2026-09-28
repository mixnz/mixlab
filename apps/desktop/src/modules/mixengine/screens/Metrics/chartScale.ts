import { formatBytes, formatCpu } from "../../metricsState";

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const MIB = 1024 * 1024;

/**
 * Một đại lượng biết tự dựng thang dọc và tự viết nhãn cho mình.
 *
 * Biểu đồ không biết nó đang vẽ phần trăm hay byte, và không nên biết: hai thứ đó làm tròn theo hai
 * cơ số khác nhau (10 và 2) và đọc ra hai kiểu khác nhau. Đây là chỗ duy nhất giữ đúng cặp đó.
 */
export interface Unit {
  /** Đỉnh của trục dọc cho một giá trị lớn nhất đã đo — luôn là một mốc đọc được, và không bao giờ
   *  thấp hơn sàn của đại lượng. */
  niceMax(peak: number): number;
  /** Nhãn một vạch lưới. Ngắn — nó nằm cạnh biểu đồ, không nằm trong tooltip. */
  tick(value: number): string;
  /** Giá trị đầy đủ trong tooltip, đúng độ chính xác daemon gửi. */
  value(value: number): string;
}

/**
 * Làm tròn `value` lên mốc `steps` gần nhất trên cơ số `base`.
 *
 * `steps` là phần định trị trong `[1, base)`, tăng dần, và mục cuối phải là chính `base` — một giá
 * trị nằm ngay dưới bậc trên cùng vẫn phải có chỗ để trèo lên.
 */
function niceCeil(value: number, base: number, steps: readonly number[]): number {
  if (value <= 0) return steps[0]! * Math.pow(base, 0);
  const exponent = Math.floor(Math.log(value) / Math.log(base));
  const magnitude = Math.pow(base, exponent);
  const mantissa = value / magnitude;
  const step = steps.find((candidate) => candidate >= mantissa - 1e-9) ?? steps[steps.length - 1]!;
  return step * magnitude;
}

/**
 * **Thang có sàn, không phải thang tự do.** Một daemon nhàn rỗi ở 0,03% mà thang chạy tới 0,05% thì
 * nhiễu đo đạc vẽ thành núi non; sàn giữ cho một đường phẳng trông phẳng. Từ T190c biểu đồ vẽ theo
 * phần trăm **cả máy** như Task Manager, nên sàn là 5% của máy chứ không còn là một phần tư lõi: đủ
 * nhỏ để một service dùng vài phần trăm vẫn thấy rõ hình dạng, đủ lớn để nhiễu không thành núi.
 */
const CPU_FLOOR = 5;

/** Cùng lý do, phía byte: dưới 64 MB thì cái đang nhìn là nhiễu cấp phát, không phải mức dùng. */
const RSS_FLOOR = 64 * MIB;

/** Cơ số 10, đủ dày để `250` không bị làm tròn lên `500`. */
const PERCENT_STEPS = [1, 1.5, 2, 2.5, 3, 4, 5, 6, 8, 10];

/** Cơ số 2 — mốc byte phải là mốc `formatBytes` viết ra gọn: `320 MB`, không phải `312.5 MB`. */
const BYTE_STEPS = [1, 1.25, 1.5, 2];

export const CPU_UNIT: Unit = {
  niceMax: (peak) => Math.max(CPU_FLOOR, niceCeil(peak, 10, PERCENT_STEPS)),
  // Hai chữ số thập phân rồi bỏ số 0 thừa: `25`, `2.5`, `125` — không phải `25.0000%` của tooltip.
  tick: (value) => `${Number(value.toFixed(2))}%`,
  // Giá trị đã là phần trăm cả máy (Metrics.tsx chia sẵn), nên mẫu số ở đây là 1 (T190c).
  value: (share) => formatCpu(share, 1),
};

export const RSS_UNIT: Unit = {
  niceMax: (peak) => Math.max(RSS_FLOOR, niceCeil(peak, 2, BYTE_STEPS)),
  tick: formatBytes,
  value: formatBytes,
};

/** Các bề rộng cửa sổ được phép, tăng dần. */
const WINDOWS = [HOUR, 3 * HOUR, 6 * HOUR, 12 * HOUR, 24 * HOUR];

/**
 * Đầu cửa sổ trục ngang: bậc nhỏ nhất còn chứa hết dữ liệu, nhiều nhất là cả thời gian lưu trữ.
 *
 * **Cửa sổ vừa dữ liệu, nhưng không bằng dữ liệu.** Hai thái cực đều sai. Kéo giãn đúng khoảng đã
 * đo ra hết bề rộng là cái sai bản đầu mắc phải: 40 phút trông y hệt 24 giờ, không nhãn nào cải
 * chính. Mà đóng cứng 24 giờ thì một home vừa bật có nửa tiếng số liệu chỉ được một vệt 2% bề rộng
 * — thật, nhưng không đọc được. Bậc kế trên là chỗ cả hai cùng đúng: nhãn thời gian nói đúng mình
 * đang ở đâu, phần chưa ai đo vẫn hiện ra là phần chưa ai đo, và dữ liệu vẫn đủ to để nhìn.
 *
 * `earliest` là phút sớm nhất có dòng, hoặc `null` khi chưa có dòng nào.
 */
export function windowStart(earliest: number | null, to: number, retention: number): number {
  if (earliest === null) return to - retention;
  const span = to - earliest;
  const fitted = WINDOWS.find((window) => window >= span) ?? span;
  return to - Math.max(span, Math.min(fitted, retention));
}

/** Các bước thời gian được phép, tăng dần. Mọi bước dưới một giờ là ước của một giờ, mọi bước từ
 *  một giờ trở lên là ước của một ngày — điều kiện để gióng theo mốc tròn của lịch địa phương. */
const INTERVALS = [
  MINUTE,
  2 * MINUTE,
  5 * MINUTE,
  10 * MINUTE,
  15 * MINUTE,
  30 * MINUTE,
  HOUR,
  2 * HOUR,
  3 * HOUR,
  6 * HOUR,
  12 * HOUR,
  24 * HOUR,
];

/**
 * Mốc đầu tiên từ `time` trở đi, gióng theo giờ/phút tròn của **lịch địa phương**.
 *
 * Gióng bằng các trường của `Date` chứ không bằng số học trên mốc epoch: một múi giờ lệch nửa giờ
 * (Ấn Độ, Nepal) làm `Math.ceil(time / interval) * interval` rơi vào 10:30, 11:30 — đúng khoảng
 * cách, sai mốc.
 */
function alignUp(time: number, interval: number): number {
  const at = new Date(time);
  if (at.getSeconds() !== 0 || at.getMilliseconds() !== 0) {
    at.setSeconds(0, 0);
    at.setMinutes(at.getMinutes() + 1);
  }
  if (interval < HOUR) {
    const step = interval / MINUTE;
    at.setMinutes(Math.ceil(at.getMinutes() / step) * step);
  } else {
    at.setMinutes(0);
    const step = interval / HOUR;
    at.setHours(Math.ceil(at.getHours() / step) * step);
  }
  return at.getTime();
}

/** Mốc kế tiếp, cũng bằng các trường của `Date`: một ngày đổi giờ mùa hè không dài 24 tiếng. */
function nextTick(time: number, interval: number): number {
  const at = new Date(time);
  if (interval < HOUR) at.setMinutes(at.getMinutes() + interval / MINUTE);
  else at.setHours(at.getHours() + interval / HOUR);
  return at.getTime();
}

/**
 * Các mốc thời gian tròn trong `[from, to]`, nhiều nhất `max` mốc.
 *
 * Bước được chọn là bước nhỏ nhất còn vừa `max` — một cửa sổ 24 giờ ra mốc 6 tiếng, một cửa sổ 20
 * phút ra mốc 5 phút. Cửa sổ ngắn không được để trục trống: dữ liệu mới chạy được nửa tiếng vẫn
 * phải nói lên nó đang nằm ở nửa tiếng nào.
 */
export function timeTicks(from: number, to: number, max: number): number[] {
  if (to <= from || max < 1) return [];
  const span = to - from;
  // `max` mốc bọc lấy `max - 1` khoảng: một cửa sổ dài đúng bốn bước có năm mốc, không phải bốn.
  const interval =
    INTERVALS.find((candidate) => span / candidate <= max - 1) ?? INTERVALS[INTERVALS.length - 1]!;

  const ticks: number[] = [];
  let at = alignUp(from, interval);
  while (at <= to && ticks.length < max) {
    ticks.push(at);
    const after = nextTick(at, interval);
    if (after <= at) break;
    at = after;
  }
  return ticks;
}
