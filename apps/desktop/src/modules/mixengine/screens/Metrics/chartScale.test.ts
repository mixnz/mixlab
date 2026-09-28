import { describe, expect, it } from "vitest";

import { CPU_UNIT, RSS_UNIT, timeTicks, windowStart } from "./chartScale";

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const MIB = 1024 * 1024;

describe("CPU_UNIT.niceMax", () => {
  /* Một subject nhàn rỗi không được phóng nhiễu lên thành núi: thang có sàn của nó, từ T190c là
     5% của cả máy. */
  it("never goes below its floor", () => {
    expect(CPU_UNIT.niceMax(0)).toBe(5);
    expect(CPU_UNIT.niceMax(3.2)).toBe(5);
  });

  it("rounds up to a readable step", () => {
    expect(CPU_UNIT.niceMax(42)).toBe(50);
    expect(CPU_UNIT.niceMax(101)).toBe(150);
  });

  /* `cpu_percent` là tổng theo lõi — 250 là hai lõi rưỡi, không phải một lỗi. */
  it("handles a figure past one core", () => {
    expect(CPU_UNIT.niceMax(250)).toBe(250);
  });

  /* Mỗi mốc lưới là một nửa của mốc trên nó, nên nửa thang cũng phải đọc được. */
  it("halves to a readable step too", () => {
    expect(CPU_UNIT.tick(CPU_UNIT.niceMax(250) / 2)).toBe("125%");
  });
});

describe("CPU_UNIT labels", () => {
  it("writes a tick without trailing zeroes", () => {
    expect(CPU_UNIT.tick(25)).toBe("25%");
    expect(CPU_UNIT.tick(2.5)).toBe("2.5%");
    expect(CPU_UNIT.tick(0)).toBe("0%");
  });

  /* Tooltip đọc như Task Manager, như bảng Dashboard (T190c): một chữ số thập phân của cả máy. */
  it("writes a tooltip value with one decimal, as Task Manager does", () => {
    expect(CPU_UNIT.value(12.3456789)).toBe("12.3%");
    expect(CPU_UNIT.value(0.02)).toBe("<0.1%");
  });
});

describe("RSS_UNIT.niceMax", () => {
  it("never goes below its floor", () => {
    expect(RSS_UNIT.niceMax(0)).toBe(64 * MIB);
    expect(RSS_UNIT.niceMax(30 * MIB)).toBe(64 * MIB);
  });

  /* Byte làm tròn theo luỹ thừa hai, nên nhãn ra "320 MB" chứ không phải "312.5 MB". */
  it("rounds up to a power-of-two step", () => {
    expect(RSS_UNIT.niceMax(300 * MIB)).toBe(320 * MIB);
    expect(RSS_UNIT.niceMax(700 * MIB)).toBe(768 * MIB);
  });

  it("halves to a readable step too", () => {
    expect(RSS_UNIT.tick(RSS_UNIT.niceMax(300 * MIB) / 2)).toBe("160 MB");
  });
});

describe("timeTicks", () => {
  it("puts a whole day's ticks on round hours", () => {
    const to = new Date(2026, 8, 15, 14, 37, 12, 500).getTime();
    const from = to - 24 * HOUR;
    const ticks = timeTicks(from, to, 6);

    expect(ticks.length).toBeGreaterThan(0);
    expect(ticks.length).toBeLessThanOrEqual(6);
    for (const tick of ticks) {
      expect(tick).toBeGreaterThanOrEqual(from);
      expect(tick).toBeLessThanOrEqual(to);
      const at = new Date(tick);
      expect(at.getSeconds()).toBe(0);
      expect(at.getMinutes()).toBe(0);
      expect(at.getHours() % 6).toBe(0);
    }
  });

  /* Cửa sổ ngắn rơi xuống mốc phút — trục không được rỗng chỉ vì mới có 20 phút dữ liệu. */
  it("falls to a minute interval in a short window", () => {
    const to = new Date(2026, 8, 15, 10, 23, 0, 0).getTime();
    const from = to - 20 * MINUTE;
    const ticks = timeTicks(from, to, 6);

    expect(ticks).toHaveLength(4);
    for (const tick of ticks) expect(new Date(tick).getMinutes() % 5).toBe(0);
  });

  it("is empty for an inverted window", () => {
    const now = Date.now();
    expect(timeTicks(now, now - HOUR, 6)).toEqual([]);
  });
});

describe("windowStart", () => {
  const now = 1_000 * HOUR;
  const day = 24 * HOUR;

  /* Nửa tiếng dữ liệu ép vào khung 24 giờ là một vệt 2% bề rộng — đúng sự thật mà không đọc được
     gì. Cửa sổ co lại tới bậc vừa đủ chứa, và phần chưa đo vẫn hiện ra là phần chưa đo. */
  it("shrinks to the smallest step that holds the data", () => {
    expect(windowStart(now - 20 * MINUTE, now, day)).toBe(now - HOUR);
    expect(windowStart(now - 4 * HOUR, now, day)).toBe(now - 6 * HOUR);
  });

  it("opens to the whole retention window once the data fills it", () => {
    expect(windowStart(now - 20 * HOUR, now, day)).toBe(now - day);
  });

  it("uses the retention window when there is no data", () => {
    expect(windowStart(null, now, day)).toBe(now - day);
  });

  /* Một home giữ lịch sử ngắn hơn số liệu nó đang có vẫn phải vẽ hết số liệu đó. */
  it("never cuts off data older than the retention window", () => {
    expect(windowStart(now - 20 * HOUR, now, 6 * HOUR)).toBe(now - 20 * HOUR);
  });
});
