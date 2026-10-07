import type { MetricsFrame } from "@mixengine/api";
import type { MetricsSample } from "@mixengine/api";

/**
 * Joins a `ServiceId` into exactly the `MetricsSubject` string MixEngine uses on the wire.
 *
 * `"service:<id>"` — the `service:` prefix is load-bearing, not decoration: `ServiceId::parse`
 * accepts bare names, so a service may perfectly well be named `daemon`; sharing one spelling would
 * attribute the daemon's history to that service. The type exported to TypeScript is deliberately a
 * bare `String` (`ts(as = "String")`, `mixengine-proto/src/metrics.rs`) — this grammar only exists
 * in `MetricsSubject::parse` on the Rust side and cannot be checked from the data type, so the
 * client has to keep it right in exactly one place.
 */
export function metricsSubjectFor(serviceId: string): string {
  return `service:${serviceId}`;
}

/** The daemon's own subject — it has no matching `ServiceRow` and is drawn apart from the service
 *  table. */
export const DAEMON_SUBJECT = "daemon";

/**
 * Parses one raw message from `/metrics`. `null` if it is not a valid `MetricsFrame`.
 *
 * A frame from a daemon older than T190c has no `cores`: its figure was already a percentage of
 * one core, so `cores` defaults to 1 instead of rejecting the whole frame.
 */
export function parseMetricsFrame(raw: string): MetricsFrame | null {
  try {
    const value = JSON.parse(raw) as { at?: unknown; samples?: unknown; cores?: unknown };
    if (typeof value.at !== "number" || !Array.isArray(value.samples)) return null;
    const cores = typeof value.cores === "number" ? value.cores : 1;
    return { ...(value as unknown as MetricsFrame), cores };
  } catch {
    return null;
  }
}

/**
 * A subject's sample in the latest frame, or `null` if that subject is absent.
 *
 * **Absent from the frame is not 0** — a subject that could not be measured is a subject not in
 * `samples`, not a `MetricsSample` of zeros (`MetricsFrame` doc comment). Calling this instead of
 * doing your own `find` is the only thing that keeps that rule.
 */
export function readingFor(frame: MetricsFrame | null, subject: string): MetricsSample | null {
  if (frame === null) return null;
  return frame.samples.find((sample) => sample.subject === subject) ?? null;
}

/**
 * Every service in `frame` added up, as one sample — the tray panel's *Services* strip (T168).
 *
 * Only what the frame measured is added: a service missing from it is not counted as 0, and a
 * service whose CPU could not be read adds its memory and nothing to the CPU. `null` when the
 * frame measured no service at all, and a `null` CPU when it measured no service's CPU — the same
 * "absent is not zero" rule as `readingFor`.
 */
export function servicesTotal(frame: MetricsFrame | null): MetricsSample | null {
  if (frame === null) return null;
  const services = frame.samples.filter((sample) => sample.subject.startsWith("service:"));
  if (services.length === 0) return null;
  const cpus = services.flatMap((sample) => (sample.cpu_percent === null ? [] : [sample.cpu_percent]));
  return {
    subject: "services",
    cpu_percent: cpus.length === 0 ? null : cpus.reduce((sum, cpu) => sum + cpu, 0),
    rss_bytes: services.reduce((sum, sample) => sum + sample.rss_bytes, 0),
    processes: services.reduce((sum, sample) => sum + sample.processes, 0),
  };
}

const BYTE_UNITS = ["B", "KB", "MB", "GB", "TB"];

/** A size readable at a glance. One decimal place, dropped when the number is whole. */
export function formatBytes(bytes: number): string {
  let size = bytes;
  let unit = 0;
  while (size >= 1024 && unit < BYTE_UNITS.length - 1) {
    size /= 1024;
    unit++;
  }
  const shown = unit === 0 ? String(size) : size.toFixed(1).replace(/\.0$/, "");
  return `${shown} ${BYTE_UNITS[unit]}`;
}

/**
 * `cpu_percent` (a percentage of one core) converted to a percentage of the whole machine — T190c.
 *
 * The denominator is the logical thread count the daemon sends along (`MetricsFrame.cores`,
 * `MetricsHistory.cores`), which is also the denominator Task Manager uses. `cores` of 0 is
 * treated as 1.
 */
export function machineShare(percentOfOneCore: number, cores: number): number {
  return percentOfOneCore / Math.max(1, cores);
}

/** What a CPU percentage is a percentage of: the whole machine, as Task Manager shows it, or one
 *  core, as `top` does — where a process busy on two cores reads 200%. Settings → MixEngine. */
export type CpuScale = "machine" | "core";

/** `cpu_percent` (a share of one core) on the chosen scale. */
export function cpuShare(percentOfOneCore: number, cores: number, scale: CpuScale): number {
  return scale === "core" ? percentOfOneCore : machineShare(percentOfOneCore, cores);
}

/**
 * CPU with one decimal place, on the chosen scale — the whole machine unless told otherwise.
 *
 * Rounds the way the eye expects at the bottom: above 0.05 is at least `0.1%`, so a working process
 * never reads as idle, and 0.05 or less is `0.0%`. Not measured yet shows `—`.
 */
export function formatCpu(percentOfOneCore: number | null, cores: number, scale: CpuScale = "machine"): string {
  if (percentOfOneCore === null) return "—";
  const share = cpuShare(percentOfOneCore, cores, scale);
  if (share <= 0.05) return "0.0%";
  if (share < 0.1) return "0.1%";
  return `${share.toFixed(1)}%`;
}
