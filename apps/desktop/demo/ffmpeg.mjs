/** Running ffmpeg for `demo/clip.mjs`: what to run is `encode.mjs`'s business, running it is this. */
import { spawn } from "node:child_process";
import { once } from "node:events";
import { resample, videoArgs } from "./encode.mjs";

/**
 * Feeds ffmpeg one PNG per tick on stdin, waiting whenever the pipe is full.
 *
 * An ffmpeg that quits early closes the pipe under the writes; that is reported as its exit code
 * and first line of stderr — what says why — never as the pipe error it leaves behind.
 */
export async function writeVideo(ffmpeg, frames, end, outPath) {
  const child = spawn(ffmpeg, videoArgs(outPath), { stdio: ["pipe", "ignore", "pipe"] });
  let stderr = "";
  child.stderr.on("data", (chunk) => (stderr += chunk));
  const exited = once(child, "close");
  let broken = false;
  child.stdin.on("error", () => (broken = true));
  // Settles on a drain, a broken pipe or the child closing — never rejects, so the exit code is
  // what gets reported, and a pipe that will never drain cannot hang the run.
  const drainedOrGone = () =>
    new Promise((resolve) => {
      const settle = () => {
        child.stdin.off("drain", settle);
        child.stdin.off("error", settle);
        child.off("close", settle);
        resolve();
      };
      child.stdin.on("drain", settle);
      child.stdin.on("error", settle);
      child.on("close", settle);
    });

  for (const index of resample(frames, end)) {
    if (broken || child.exitCode !== null) break;
    if (!child.stdin.write(frames[index].data)) await drainedOrGone();
  }
  child.stdin.end();
  const [code] = await exited;
  if (code !== 0) throw new Error(`ffmpeg exited with ${code}: ${stderr.trim().split("\n")[0]}`);
}
