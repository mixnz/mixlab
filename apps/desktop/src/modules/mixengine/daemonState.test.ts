import { describe, expect, it } from "vitest";

import {
  applyEvent,
  applyJob,
  movesARow,
  needsResync,
  rowsFrom,
  type JobRow,
  stoppedByReason,
  type ServiceRow,
} from "./daemonState";

const rows: ServiceRow[] = [
  { id: "mariadb@main", state: "running", port: 3306, autostart: true, stoppedBy: null, version: "11.4.3" },
  { id: "caddy@main", state: "stopped", port: null, autostart: false, stoppedBy: null, version: null },
];

describe("rowsFrom", () => {
  /* The `autostart` column is a **setting**, not a state: it comes from `service.list` and no event
     changes it. This test is what keeps it from being forgotten when someone adds a field to
     `ServiceRow`. */
  it("carries a service's autostart setting onto its row", () => {
    const made = rowsFrom([
      {
        id: "redis@main",
        state: "stopped",
        supervised: false,
        pid: null,
        last_started_at: null,
        last_exit_code: null,
        depends_on: [],
        autostart: true,
      },
    ]);

    expect(made).toEqual([
      { id: "redis@main", state: "stopped", port: null, autostart: true, stoppedBy: null, version: null },
    ]);
  });

  /* T183: the version is what the user reads next to the id. An old daemon does not send this
     field, so it becomes `null`, and `null` means draw nothing. */
  it("carries a service's version onto its row, and null when the daemon sent none", () => {
    const base = {
      id: "mysql@main",
      state: "stopped" as const,
      supervised: false,
      pid: null,
      last_started_at: null,
      last_exit_code: null,
      depends_on: [],
      autostart: false,
    };
    expect(rowsFrom([{ ...base, version: "5.7.44" }])[0].version).toBe("5.7.44");
    expect(rowsFrom([base])[0].version).toBeNull();
  });
});

describe("applyEvent", () => {
  /* State is announced, never inferred: a row changes because the stream says so, not because
     someone clicked. */
  it("moves a row when the stream says the service changed", () => {
    const raw = JSON.stringify({ type: "service_state_changed", service: "caddy@main", to: "starting" });
    const next = applyEvent(rows, raw);
    expect(next.rows.find((r) => r.id === "caddy@main")?.state).toBe("starting");
    expect(next.rows.find((r) => r.id === "mariadb@main")?.state).toBe("running");
    expect(next.resync).toBe(false);
  });

  /* `resync` means the 1024-message bus on the other side has overflowed. The `missed` count is
     only for logging — the handling is the same whether one or a thousand were missed. */
  it("asks for a resync when the bus overflowed", () => {
    expect(applyEvent(rows, JSON.stringify({ type: "resync", missed: 900 })).resync).toBe(true);
  });

  it("asks for a resync when the connection dropped", () => {
    expect(applyEvent(rows, JSON.stringify({ type: "mixlab_disconnected" })).resync).toBe(true);
  });

  /* A variant born in a later version must arrive here as an ignorable object — no throw, and no
     rows lost. That is the whole reason events are internally tagged. */
  it("ignores an event type it has never heard of", () => {
    const next = applyEvent(rows, JSON.stringify({ type: "quantum_flux", whatever: 1 }));
    expect(next.rows).toEqual(rows);
    expect(next.resync).toBe(false);
  });

  it("ignores something that is not even JSON", () => {
    expect(applyEvent(rows, "<html>").rows).toEqual(rows);
  });

  /* A service not yet in the table: build no fake row; wait for `service.list` to say what it
     is. */
  it("does not invent a row for a service it does not know", () => {
    const raw = JSON.stringify({ type: "service_state_changed", service: "redis@main", to: "running" });
    expect(applyEvent(rows, raw).rows).toHaveLength(2);
  });

  /* A `service_state_changed` missing either half must not touch the table. */
  it("ignores a state change that names no service or no state", () => {
    expect(applyEvent(rows, JSON.stringify({ type: "service_state_changed", to: "running" })).rows)
      .toEqual(rows);
    expect(
      applyEvent(rows, JSON.stringify({ type: "service_state_changed", service: "caddy@main" }))
        .rows,
    ).toEqual(rows);
  });

  /* Pins down the exact mistake that was made: `id` is the name in MixEngine's architecture sketch,
     `service` is the name the daemon actually sends. Misreading it leaves the table frozen with
     nothing to say so. */
  it("does not answer to the field name the architecture note used", () => {
    const raw = JSON.stringify({ type: "service_state_changed", id: "caddy@main", to: "running" });
    expect(applyEvent(rows, raw).rows).toEqual(rows);
  });
});

describe("applyJob", () => {
  const running: JobRow[] = [{ id: 7, kind: "elevation", percent: 20, message: "asking" }];

  it("adds a job the first time it reports", () => {
    const raw = JSON.stringify({
      type: "job_progress",
      job: 9,
      kind: "install",
      percent: 5,
      message: "downloading",
    });
    const next = applyJob([], raw);
    expect(next).toHaveLength(1);
    expect(next[0]).toMatchObject({ id: 9, percent: 5, message: "downloading" });
  });

  /* Progress is the only thing on the stream allowed to repeat — it updates the existing row and
     spawns no new one. */
  it("updates a job it already has instead of adding a second row", () => {
    const raw = JSON.stringify({ type: "job_progress", job: 7, percent: 80, message: "granted" });
    const next = applyJob(running, raw);
    expect(next).toHaveLength(1);
    expect(next[0].percent).toBe(80);
  });

  /* `kind` is only on the first message; a later message must not erase it. */
  it("keeps the kind a later message does not repeat", () => {
    const raw = JSON.stringify({ type: "job_progress", job: 7, percent: 80 });
    expect(applyJob(running, raw)[0].kind).toBe("elevation");
  });

  it("takes a finished job off the list", () => {
    expect(applyJob(running, JSON.stringify({ type: "job_finished", job: 7 }))).toHaveLength(0);
  });

  it("leaves the list alone for anything else", () => {
    expect(applyJob(running, JSON.stringify({ type: "resync", missed: 2 }))).toEqual(running);
    expect(applyJob(running, "not json")).toEqual(running);
    expect(applyJob(running, JSON.stringify({ type: "job_progress" }))).toEqual(running);
  });
});

describe("needsResync", () => {
  /* The same answer as `applyEvent`, but callable outside a `setState` updater — React runs
     updaters twice under StrictMode, so a `reload()` placed inside one fires twice per event. */
  it("says yes to exactly what applyEvent says yes to", () => {
    for (const raw of [
      JSON.stringify({ type: "resync", missed: 1 }),
      JSON.stringify({ type: "mixlab_disconnected" }),
    ]) {
      expect(needsResync(raw)).toBe(true);
      expect(applyEvent(rows, raw).resync).toBe(true);
    }
    for (const raw of [
      JSON.stringify({ type: "service_state_changed", service: "caddy@main", to: "running" }),
      JSON.stringify({ type: "quantum_flux" }),
      "not json",
    ]) {
      expect(needsResync(raw)).toBe(false);
      expect(applyEvent(rows, raw).resync).toBe(false);
    }
  });
});

describe("movesARow", () => {
  /* Only `service_state_changed` changes the service table. This question exists so `Dashboard`
     knows whether a message races a `service.list` in flight — asked **outside** the `setRows`
     updater, because updaters run twice under StrictMode. */
  it("says yes to a service state change", () => {
    const raw = JSON.stringify({ type: "service_state_changed", service: "caddy@main", to: "stopped" });
    expect(movesARow(raw)).toBe(true);
  });

  /* Job progress fires continuously throughout a runtime install. Treating it as a reason to reread
     turns one long job into a barrage of `service.list` calls nobody needs. */
  it("says no to job progress", () => {
    expect(movesARow(JSON.stringify({ type: "job_progress", job: 4, percent: 10 }))).toBe(false);
  });

  it("says no to something it cannot parse", () => {
    expect(movesARow("not json")).toBe(false);
  });
});

/* T167g: who stopped a service rides on the transition's reason, by the same rule MixEngine's
   `StoppedBy::of` uses — so the Dashboard can draw an idle stop as resting without a second read. */
describe("stoppedBy from a transition", () => {
  const stop = (reason: unknown) =>
    applyEvent(
      rows,
      JSON.stringify({ type: "service_state_changed", service: "mariadb@main", to: "stopped", reason }),
    ).rows.find((row) => row.id === "mariadb@main")?.stoppedBy;

  it("is the daemon for an idle stop and a person for a requested one", () => {
    expect(stop({ kind: "idle", after: 60000 })).toBe("daemon");
    expect(stop({ kind: "requested" })).toBe("person");
    expect(stop({ kind: "credential_reset" })).toBe("person");
  });

  it("is unknown when the reason cannot be read, and cleared by a start", () => {
    expect(stop(undefined)).toBeNull();
    expect(stoppedByReason({ kind: 3 })).toBeNull();

    const started = applyEvent(
      [{ id: "caddy@main", state: "stopped", port: null, autostart: false, stoppedBy: "daemon", version: null }],
      JSON.stringify({ type: "service_state_changed", service: "caddy@main", to: "starting", reason: { kind: "requested" } }),
    ).rows[0];
    expect(started.stoppedBy).toBeNull();
  });
});
