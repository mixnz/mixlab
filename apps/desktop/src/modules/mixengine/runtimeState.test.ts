import { describe, expect, it } from "vitest";

import {
  formatInstalledAt,
  installedVersions,
  newestFirst,
  jobFinished,
  jobFor,
  poolBanner,
  versionKey,
} from "./runtimeState";
import type { JobRow } from "./daemonState";

describe("versionKey", () => {
  it("joins kind and version with @", () => {
    expect(versionKey("php", "8.3.12")).toBe("php@8.3.12");
  });
});

describe("poolBanner", () => {
  it("maps reloaded to no banner", () => {
    expect(poolBanner("reloaded")).toBe("none");
  });
  it("maps restart_required to a restart banner", () => {
    expect(poolBanner("restart_required")).toBe("restartRequired");
  });
  it("maps pool_not_running to an apply-next-start message, not an error", () => {
    expect(poolBanner("pool_not_running")).toBe("appliesNextStart");
  });
});

describe("formatInstalledAt", () => {
  it("turns an epoch-millisecond Timestamp into the machine's own date/time, not a raw number", () => {
    const ms = Date.UTC(2026, 0, 15, 12, 0, 0);
    const formatted = formatInstalledAt(ms);
    expect(formatted).not.toBe(String(ms));
    expect(formatted).toBe(new Date(ms).toLocaleString());
  });
});

describe("jobFor", () => {
  const jobs: JobRow[] = [{ id: 1, kind: "runtime.install", percent: 40, message: "downloading" }];

  it("finds the job tracked for a version", () => {
    expect(jobFor(jobs, 1)).toEqual(jobs[0]);
  });

  it("returns undefined when nothing is tracked yet", () => {
    expect(jobFor(jobs, undefined)).toBeUndefined();
  });

  it("returns undefined once the job has finished and left the list", () => {
    expect(jobFor(jobs, 2)).toBeUndefined();
  });
});

describe("jobFinished", () => {
  it("reads the job id off a job_finished message", () => {
    expect(jobFinished(JSON.stringify({ type: "job_finished", job: 7, ending: "succeeded" }))?.id).toBe(
      7,
    );
  });

  it("carries no error for a job that succeeded", () => {
    const finished = jobFinished(
      JSON.stringify({ type: "job_finished", job: 7, ending: "succeeded", result: null }),
    );
    expect(finished?.error).toBeNull();
  });

  // A failed job is the whole reason this function reads `ending`: it is the only place the
  // daemon's sentence reaches the user, and the previous version dropped it — the progress bar
  // vanished, the list did not change, and not a word explained why.
  it("turns a failed job into the same refusal a rejected call would have carried", () => {
    const finished = jobFinished(
      JSON.stringify({
        type: "job_finished",
        job: 7,
        ending: "failed",
        error: {
          code: "precondition_failed",
          message: "archive contains ./, which is not inside it",
          hint: "nothing was unpacked",
        },
      }),
    );
    expect(finished?.error).toEqual({
      code: "error.mixengineRefused",
      params: {
        code: "precondition_failed",
        message: "archive contains ./, which is not inside it",
        hint: "nothing was unpacked",
      },
    });
  });

  it("leaves out the hint a failure did not carry, rather than sending an empty one", () => {
    const finished = jobFinished(
      JSON.stringify({
        type: "job_finished",
        job: 7,
        ending: "failed",
        error: { code: "io", message: "disk full" },
      }),
    );
    expect(finished?.error?.params).not.toHaveProperty("hint");
  });

  // If the user cancelled it themselves there is nothing to report — a red banner for something
  // they just asked for is noise.
  it("carries no error for a cancelled job", () => {
    expect(
      jobFinished(JSON.stringify({ type: "job_finished", job: 7, ending: "cancelled" }))?.error,
    ).toBeNull();
  });

  it("returns null for a job_progress message", () => {
    expect(jobFinished(JSON.stringify({ type: "job_progress", job: 7, percent: 40 }))).toBeNull();
  });

  it("returns null for an unrelated message", () => {
    expect(jobFinished(JSON.stringify({ type: "service_state_changed" }))).toBeNull();
  });

  it("returns null for invalid JSON", () => {
    expect(jobFinished("not json")).toBeNull();
  });
});

describe("installedVersions", () => {
  function runtime(kind: string, version: string) {
    return { kind, version } as Parameters<typeof installedVersions>[0][number];
  }

  it("keeps only the kind that was asked for", () => {
    const list = [runtime("php", "8.3.12"), runtime("node", "20.11.1")];
    expect(installedVersions(list, "php")).toEqual(["8.3.12"]);
  });

  it("answers nothing for a kind with nothing installed", () => {
    expect(installedVersions([runtime("php", "8.3.12")], "ruby")).toEqual([]);
  });

  /* The daemon answers `ORDER BY kind, version`, which is a string sort — so it hands back `8.10`
     before `8.9`. Newest first only means anything if the segments are compared as numbers. */
  it("puts a higher segment first even when it is the shorter string", () => {
    const list = [runtime("php", "8.9.0"), runtime("php", "8.10.0")];
    expect(installedVersions(list, "php")).toEqual(["8.10.0", "8.9.0"]);
  });

  it("orders newest first across major, minor and patch", () => {
    const list = [
      runtime("php", "8.2.23"),
      runtime("php", "7.4.33"),
      runtime("php", "8.3.12"),
      runtime("php", "8.3.9"),
    ];
    expect(installedVersions(list, "php")).toEqual(["8.3.12", "8.3.9", "8.2.23", "7.4.33"]);
  });

  /* A constraint naming no pre-release never selects one, so a release candidate is the older of
     the two and belongs below the release it precedes. */
  it("puts a release above the release candidate that precedes it", () => {
    const list = [runtime("php", "8.5.0RC1"), runtime("php", "8.5.0")];
    expect(installedVersions(list, "php")).toEqual(["8.5.0", "8.5.0RC1"]);
  });

  it("treats a version with fewer segments as the earlier one", () => {
    const list = [runtime("node", "20.11"), runtime("node", "20.11.1")];
    expect(installedVersions(list, "node")).toEqual(["20.11.1", "20.11"]);
  });
});

describe("newestFirst", () => {
  function row(name: string, version: string) {
    return { name, version };
  }

  it("orders each name's versions newest first, comparing segments as numbers", () => {
    const rows = [
      row("php", "8.9.0"),
      row("php", "8.10.0"),
      row("php", "8.5.0RC1"),
      row("php", "8.5.0"),
    ];
    expect(newestFirst(rows, (r) => r.name).map((r) => r.version)).toEqual([
      "8.10.0",
      "8.9.0",
      "8.5.0",
      "8.5.0RC1",
    ]);
  });

  /* The daemon's order between names is kept: only versions of one name move, and two names are
     never interleaved even when their rows arrived mixed. */
  it("keeps names in the order they first appear and groups their rows", () => {
    const rows = [
      row("php", "8.2.0"),
      row("node", "20.0.0"),
      row("php", "8.3.0"),
      row("node", "22.0.0"),
    ];
    expect(newestFirst(rows, (r) => r.name)).toEqual([
      row("php", "8.3.0"),
      row("php", "8.2.0"),
      row("node", "22.0.0"),
      row("node", "20.0.0"),
    ]);
  });

  it("does not touch the list it is given", () => {
    const rows = [row("php", "8.2.0"), row("php", "8.3.0")];
    newestFirst(rows, (r) => r.name);
    expect(rows.map((r) => r.version)).toEqual(["8.2.0", "8.3.0"]);
  });
});
