import { describe, expect, it } from "vitest";
import { BUDGET, FPS, budgetVerdict, findFfmpeg, formatMB, posterArgs, resample, videoArgs } from "./encode.mjs";

describe("resample", () => {
  it("shows, at each tick, the last frame that had arrived, and holds the last until the end", () => {
    const frames = [{ t: 10 }, { t: 10.1 }, { t: 10.25 }];
    // Ticks at 10.00, 10.10, 10.20, 10.30, 10.40 for an end at 10.45 and 10 frames a second.
    expect(resample(frames, 10.45, 10)).toEqual([0, 1, 1, 2, 2]);
  });

  it("lasts as long as the recording, whatever the frames' spacing", () => {
    const frames = [{ t: 0 }, { t: 0.01 }, { t: 2.5 }];
    expect(resample(frames, 3, 24)).toHaveLength(72);
  });

  it("refuses nothing to encode", () => {
    expect(() => resample([], 1, 24)).toThrow("no frames");
  });
});

describe("ffmpeg arguments", () => {
  it("encode a small, web-ready H.264 at the website's size", () => {
    const args = videoArgs("out.mp4");
    for (const pair of [
      ["-f", "image2pipe"],
      ["-framerate", String(FPS)],
      ["-i", "-"],
      ["-c:v", "libx264"],
      ["-crf", "26"],
      ["-pix_fmt", "yuv420p"],
      ["-movflags", "+faststart"],
    ]) {
      // The last occurrence: `-c:v` names the input codec (png) before the output one.
      expect(args[args.lastIndexOf(pair[0]) + 1]).toBe(pair[1]);
    }
    expect(args).toContain("-an");
    expect(args).toContain("scale=2080:1300:flags=lanczos");
    expect(FPS).toBe(24);
    expect(args.at(-1)).toBe("out.mp4");
  });

  it("scale a poster to the same size", () => {
    expect(posterArgs("in.png", "out.png")).toEqual([
      "-y",
      "-loglevel",
      "error",
      "-i",
      "in.png",
      "-vf",
      "scale=2080:1300:flags=lanczos",
      "out.png",
    ]);
  });
});

describe("budget", () => {
  it("is fine up to 1 MB, a warning to 1.5 MB, a failure past it", () => {
    expect(budgetVerdict(BUDGET.warn)).toBe("ok");
    expect(budgetVerdict(BUDGET.warn + 1)).toBe("warn");
    expect(budgetVerdict(BUDGET.fail)).toBe("warn");
    expect(budgetVerdict(BUDGET.fail + 1)).toBe("fail");
    expect(formatMB(1_234_567)).toBe("1.23 MB");
  });
});

describe("findFfmpeg", () => {
  it("prefers FFMPEG, then ffmpeg on PATH, and says null when neither runs", () => {
    expect(findFfmpeg({ FFMPEG: "C:/tools/ffmpeg.exe" }, () => true)).toBe("C:/tools/ffmpeg.exe");
    expect(findFfmpeg({}, (cmd) => cmd === "ffmpeg")).toBe("ffmpeg");
    expect(findFfmpeg({ FFMPEG: "/nope" }, (cmd) => cmd === "ffmpeg")).toBe("ffmpeg");
    expect(findFfmpeg({}, () => false)).toBeNull();
  });
});
