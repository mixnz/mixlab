import { describe, expect, it } from "vitest";
import { writeVideo } from "./ffmpeg.mjs";

describe("writeVideo", () => {
  it("reports an ffmpeg that quits early instead of crashing on the closed pipe", async () => {
    // node rejects ffmpeg's first flag and exits at once: an encoder that died mid-clip.
    const frame = { data: Buffer.alloc(1024 * 1024, 7), t: 0 };
    const frames = [frame, { ...frame, t: 10 }];
    await expect(writeVideo(process.execPath, frames, 20, "out.mp4")).rejects.toThrow(/ffmpeg exited with/);
  });
});
