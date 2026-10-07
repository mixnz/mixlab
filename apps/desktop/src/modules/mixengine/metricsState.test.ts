import { describe, expect, it } from "vitest";

import {
  DAEMON_SUBJECT,
  formatBytes,
  cpuShare,
  formatCpu,
  machineShare,
  metricsSubjectFor,
  parseMetricsFrame,
  readingFor,
  servicesTotal,
} from "./metricsState";

describe("metricsSubjectFor", () => {
  it("prefixes a service id with service:, since the prefix is load-bearing", () => {
    expect(metricsSubjectFor("mariadb@main")).toBe("service:mariadb@main");
  });

  /* A service named "daemon" is valid on MixEngine's side (ServiceId::parse accepts bare names) —
     without the prefix, it would collide with DAEMON_SUBJECT and take over the daemon's history. */
  it("does not collide with the daemon subject for a service literally named daemon", () => {
    expect(metricsSubjectFor("daemon")).not.toBe(DAEMON_SUBJECT);
    expect(metricsSubjectFor("daemon")).toBe("service:daemon");
  });
});

describe("parseMetricsFrame", () => {
  it("parses a well-formed frame", () => {
    const raw = JSON.stringify({ at: 1757203200000, samples: [], cores: 12 });
    expect(parseMetricsFrame(raw)).toEqual({ at: 1757203200000, samples: [], cores: 12 });
  });

  /* A daemon older than T190c does not send `cores`: its figure was already a percentage of one
     core. */
  it("reads an old daemon's frame as percent of one core", () => {
    const raw = JSON.stringify({ at: 1757203200000, samples: [] });
    expect(parseMetricsFrame(raw)?.cores).toBe(1);
  });

  it("returns null for something that is not even JSON", () => {
    expect(parseMetricsFrame("<html>")).toBeNull();
  });

  it("returns null for JSON missing the fields a frame must have", () => {
    expect(parseMetricsFrame(JSON.stringify({ whatever: 1 }))).toBeNull();
  });
});

describe("readingFor", () => {
  const frame = {
    at: 1757203200000,
    samples: [
      { subject: "daemon", cpu_percent: 1, rss_bytes: 100, processes: 1 },
      { subject: "service:mariadb@main", cpu_percent: null, rss_bytes: 200, processes: 2 },
    ],
    cores: 1,
  };

  it("finds the sample for a subject present in the frame", () => {
    expect(readingFor(frame, "service:mariadb@main")?.rss_bytes).toBe(200);
  });

  /* Absent from the frame is not 0 — a subject that could not be measured is not in samples; it is
     not a sample of zeros. */
  it("is null for a subject absent from the frame, not a zeroed sample", () => {
    expect(readingFor(frame, "service:caddy@main")).toBeNull();
  });

  it("is null when there is no frame yet", () => {
    expect(readingFor(null, "daemon")).toBeNull();
  });

  /* cpu_percent: null in a sample that is present must stay null through readingFor — not a parse
     error, but the real answer of a first measurement with nothing to subtract yet. */
  it("keeps a present sample's null cpu_percent as null, not coerced to zero", () => {
    expect(readingFor(frame, "service:mariadb@main")?.cpu_percent).toBeNull();
  });
});

describe("formatBytes", () => {
  it("rounds to one decimal and drops it when it would be a zero", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1024)).toBe("1 KB");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(1024 * 1024)).toBe("1 MB");
  });
});

/* T190c: `cpu_percent` is a percentage of ONE core; users read it the Task Manager way, i.e. as a
   percentage of the whole machine, with one decimal place. */
describe("formatCpu", () => {
  it("shows a share of the machine with one decimal, as Task Manager does", () => {
    expect(formatCpu(150, 12)).toBe("12.5%");
    expect(formatCpu(7.3, 12)).toBe("0.6%");
  });

  it("shows anything above 0.05 as at least 0.1, and the rest as 0.0", () => {
    expect(formatCpu(0.72, 12)).toBe("0.1%");
    expect(formatCpu(0.6, 12)).toBe("0.0%");
    expect(formatCpu(0.53, 12)).toBe("0.0%");
  });

  it("shows a share of one core when asked", () => {
    expect(formatCpu(150, 12, "core")).toBe("150.0%");
    expect(formatCpu(0.07, 12, "core")).toBe("0.1%");
  });

  it("shows zero as zero and an unmeasured figure as a dash", () => {
    expect(formatCpu(0, 12)).toBe("0.0%");
    expect(formatCpu(null, 12)).toBe("—");
  });

  it("treats a machine of no cores as one", () => {
    expect(formatCpu(50, 0)).toBe("50.0%");
  });
});

describe("cpuShare", () => {
  it("divides by the cores only on the machine scale", () => {
    expect(cpuShare(150, 12, "machine")).toBe(12.5);
    expect(cpuShare(150, 12, "core")).toBe(150);
  });
});

describe("machineShare", () => {
  it("divides a share of one core by the machine's cores", () => {
    expect(machineShare(150, 12)).toBe(12.5);
    expect(machineShare(150, 0)).toBe(150);
  });
});

describe("servicesTotal", () => {
  const sample = (subject: string, cpu: number | null, rss: number) => ({
    subject,
    cpu_percent: cpu,
    rss_bytes: rss,
    processes: 1,
  });

  it("adds every service and leaves the daemon out", () => {
    const total = servicesTotal({
      at: 1,
      cores: 1,
      samples: [sample("daemon", 5, 100), sample("service:caddy", 1.5, 10), sample("service:mariadb@main", 2, 30)],
    });
    expect(total?.cpu_percent).toBe(3.5);
    expect(total?.rss_bytes).toBe(40);
    expect(total?.processes).toBe(2);
  });

  it("does not count an unreadable CPU as zero", () => {
    const total = servicesTotal({
      at: 1,
      cores: 1,
      samples: [sample("service:caddy", null, 10), sample("service:redis@main", 1, 5)],
    });
    expect(total?.cpu_percent).toBe(1);
    expect(servicesTotal({ at: 1, cores: 1, samples: [sample("service:caddy", null, 10)] })?.cpu_percent).toBeNull();
  });

  it("is absent when no service was measured", () => {
    expect(servicesTotal({ at: 1, cores: 1, samples: [sample("daemon", 5, 100)] })).toBeNull();
    expect(servicesTotal(null)).toBeNull();
  });
});
