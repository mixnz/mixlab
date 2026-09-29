import { useEffect, useMemo, useRef, useState, type PointerEvent } from "react";

import type { MetricsMinute } from "@mixengine/api";
import Tooltip from "../../../../components/Tooltip";
import { useTranslation } from "../../../../i18n";
import { gapsIn, nearestMinute, runsOf, sampledRanges } from "../../metricsHistoryState";
import { timeTicks, type Unit } from "./chartScale";
import styles from "./Chart.module.css";

const MINUTE_MS = 60_000;

/** The height of the plot area, not counting margins. */
const PLOT_HEIGHT = 128;
/** Leaves room for the stroke right at the top of the scale: without it, the upper half of the line
 *  is cut off at the SVG edge. */
const PAD_TOP = 10;
/** Room for the sampling band, then the time labels. */
const PAD_BOTTOM = 28;
/** The sampling band: sits below the 0 line, not over the data near the bottom. */
const RAIL_TOP = 4;
const RAIL_HEIGHT = 3;
/** From how many readings per minute `peak` starts to say anything different from `avg`. */
const SAMPLED_MINIMUM = 2;
/** Room for the vertical axis labels. */
const PAD_LEFT = 58;
const PAD_RIGHT = 10;
const HEIGHT = PAD_TOP + PLOT_HEIGHT + PAD_BOTTOM;
/** Below this the axis labels overlap; the chart would rather shrink than trip over itself. */
const MIN_WIDTH = 360;
/** How many pixels one time label gets before two labels touch. */
const TICK_SPACING = 110;
/** The cursor has to be within this much to count as pointing at a minute. */
const SNAP_PX = 8;
/** An empty region narrower than this has no room for text. */
const GAP_LABEL_PX = 90;

interface Props {
  segments: MetricsMinute[][];
  /** The window the horizontal axis covers — the whole retention period, not only the part that
   *  has data. */
  from: number;
  to: number;
  /** The quantity being drawn: it builds the vertical scale and writes its own labels. */
  unit: Unit;
  /** The quantity's name, translated — for someone reading the screen rather than looking at it. */
  label: string;
  /** Reads the average/peak values from a minute — the `cpu_avg`/`cpu_peak` or `rss_avg`/`rss_peak`
   *  pair. */
  avg: (minute: MetricsMinute) => number | null;
  peak: (minute: MetricsMinute) => number | null;
  /** The categorical hue the series is drawn in — one per quantity, so two charts are told apart. */
  hue: "sky" | "purple";
}

/** The minute under the cursor, with its position in CSS pixels within the frame. */
interface Hover {
  minute: MetricsMinute;
  left: number;
}

/**
 * A peak band and an average line on a real time axis, hand-built in SVG — no chart library added
 * (Decision D4, Metrics/Settings spec).
 *
 * **Each continuous run is a `<path>` of its own.** This is the real mechanism that keeps the rule
 * "a missing minute is a gap, not a joining point": the X axis maps by *time*, not by array index,
 * so a gap in time leaves a gap in the geometry by itself — and not drawing a `<path>` through it
 * is the only thing left to get right. `runsOf` applies the same rule one level deeper, for minutes
 * that have a row but no figure.
 *
 * **The frame is measured in real pixels.** `viewBox` matches the measured width so one SVG unit
 * is one pixel: strokes are equally thick in every direction, and text comes out at the right
 * size. The previous version stretched a fixed 640×120 `viewBox` across the whole pane width with
 * `preserveAspectRatio="none"`, making horizontal stretches thinner than steep ones and turning the
 * 4px peak stroke into a coarse band.
 */
export default function Chart({ segments, from, to, unit, label, avg, peak, hue }: Props) {
  const { lang, t } = useTranslation();
  const box = useRef<HTMLDivElement>(null);
  const plot = useRef<SVGSVGElement>(null);
  const [measured, setMeasured] = useState(0);
  const [hover, setHover] = useState<Hover | null>(null);

  useEffect(() => {
    const el = box.current;
    if (el === null) return;
    const measure = () => setMeasured(el.clientWidth);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const clock = useMemo(
    () => new Intl.DateTimeFormat(lang, { hour: "2-digit", minute: "2-digit" }),
    [lang],
  );
  // The summary has to state the whole day: a screen reader cannot see how long the window is.
  const stamp = useMemo(
    () => new Intl.DateTimeFormat(lang, { dateStyle: "short", timeStyle: "short" }),
    [lang],
  );

  const width = Math.max(measured, MIN_WIDTH);
  const plotWidth = width - PAD_LEFT - PAD_RIGHT;
  const span = Math.max(1, to - from);

  const all = segments.flat();
  const observed = Math.max(0, ...all.map((m) => peak(m) ?? avg(m) ?? 0));
  const max = unit.niceMax(observed);

  const x = (time: number) => PAD_LEFT + ((time - from) / span) * plotWidth;
  const y = (value: number) => PAD_TOP + PLOT_HEIGHT - (value / max) * PLOT_HEIGHT;

  const defined = (minute: MetricsMinute) => avg(minute) !== null && peak(minute) !== null;
  const runs = segments.flatMap((segment) => runsOf(segment, defined));
  const gaps = gapsIn(segments, from, to);
  const sampled = sampledRanges(segments, SAMPLED_MINIMUM);
  const ticks = timeTicks(from, to, Math.max(2, Math.floor(plotWidth / TICK_SPACING) + 1));

  function move(event: PointerEvent<HTMLDivElement>) {
    const svg = plot.current;
    if (svg === null || all.length === 0) return;
    const rect = svg.getBoundingClientRect();
    if (rect.width === 0) return;
    const scale = width / rect.width;
    const time = from + (((event.clientX - rect.left) * scale - PAD_LEFT) / plotWidth) * span;
    const tolerance = Math.max(MINUTE_MS, (span / plotWidth) * SNAP_PX);
    const found = nearestMinute(all, time, tolerance);
    setHover(found === null ? null : { minute: found, left: (x(found.minute) / width) * rect.width });
  }

  const hovered = hover?.minute;
  const hoveredAvg = hovered === undefined ? null : avg(hovered);
  const hoveredPeak = hovered === undefined ? null : peak(hovered);

  return (
    <div className={`${styles.frame} ${styles[hue]}`}>
      <div className={styles.legend}>
        <span className={styles.key}>
          <span className={styles.keyBand} />
          {t("mixengine.metrics.peak")}
        </span>
        <span className={styles.key}>
          <span className={styles.keyLine} />
          {t("mixengine.metrics.average")}
        </span>
        {/* This legend needs an explanation, not just a name: the band says *why* the peak band is
            flat elsewhere. The app's `Tooltip` rather than `title`, so the text comes out in the
            app's font. */}
        <Tooltip text={t("mixengine.metrics.sampledHint")}>
          <span className={styles.key}>
            <span className={styles.keyRail} />
            {t("mixengine.metrics.sampled")}
          </span>
        </Tooltip>
      </div>

      <div
        className={styles.box}
        ref={box}
        onPointerMove={move}
        onPointerLeave={() => setHover(null)}
      >
        <svg
          className={styles.chart}
          ref={plot}
          viewBox={`0 0 ${width} ${HEIGHT}`}
          height={HEIGHT}
          role="img"
          aria-label={t("mixengine.metrics.summary", {
            label,
            from: stamp.format(from),
            to: stamp.format(to),
            count: all.length,
            peak: unit.value(observed),
          })}
        >
          {/* Time nobody measured, drawn as a region — otherwise it cannot be told apart from a
              flat line, and 40 minutes of data looks exactly like 24 hours. */}
          {gaps.map((gap) => (
            <rect
              key={gap.from}
              className={styles.gap}
              x={x(gap.from)}
              y={PAD_TOP}
              width={Math.max(0, x(gap.to) - x(gap.from))}
              height={PLOT_HEIGHT}
            />
          ))}
          {gaps
            .filter((gap) => x(gap.to) - x(gap.from) >= GAP_LABEL_PX)
            .map((gap) => (
              <text
                key={gap.from}
                className={styles.gapLabel}
                x={(x(gap.from) + x(gap.to)) / 2}
                y={PAD_TOP + PLOT_HEIGHT / 2}
                textAnchor="middle"
                dominantBaseline="middle"
              >
                {t("mixengine.metrics.noData")}
              </text>
            ))}

          {[0, max / 2, max].map((value) => (
            <g key={value}>
              <line
                className={styles.grid}
                x1={PAD_LEFT}
                x2={PAD_LEFT + plotWidth}
                y1={y(value)}
                y2={y(value)}
              />
              <text
                className={styles.tick}
                x={PAD_LEFT - 8}
                y={y(value)}
                textAnchor="end"
                dominantBaseline="middle"
              >
                {unit.tick(value)}
              </text>
            </g>
          ))}

          {/* The stretches someone was watching, stated below the axis rather than on the data
              line itself. `samples: 1` is the normal state of a machine where nobody has the
              Dashboard open, so fading those minutes would fade almost the whole chart — losing
              exactly what needs to be read. */}
          {sampled.map((range) => (
            <rect
              key={range.from}
              className={styles.rail}
              x={x(range.from)}
              y={PAD_TOP + PLOT_HEIGHT + RAIL_TOP}
              width={Math.max(1, x(range.to) - x(range.from))}
              height={RAIL_HEIGHT}
            />
          ))}

          {ticks.map((tick) => (
            <text
              key={tick}
              className={styles.tick}
              x={x(tick)}
              y={PAD_TOP + PLOT_HEIGHT + 20}
              textAnchor="middle"
            >
              {clock.format(tick)}
            </text>
          ))}

          {runs.map((run) => {
            const first = run[0]!;
            // A lone minute has no line to draw, but it is real data: a tick from the average up to
            // the peak plus a dot. The previous version skipped every run shorter than two points.
            if (run.length === 1) {
              return (
                <g key={first.minute}>
                  <line
                    className={styles.stem}
                    x1={x(first.minute)}
                    x2={x(first.minute)}
                    y1={y(avg(first)!)}
                    y2={y(peak(first)!)}
                  />
                  <circle className={styles.dot} cx={x(first.minute)} cy={y(avg(first)!)} r={2} />
                </g>
              );
            }
            const top = run.map((m) => `${x(m.minute)},${y(peak(m)!)}`);
            const line = run.map((m) => `${x(m.minute)},${y(avg(m)!)}`);
            // A closed band between peak and average — a *region*, not a thick stroke. The previous
            // version drew the peak with a faint `strokeWidth={4}`, so the two lines looked like
            // two separate series instead of "the average sits inside the peak region".
            const band = `M${top.join("L")}L${[...line].reverse().join("L")}Z`;
            return (
              <g key={first.minute}>
                <path className={styles.band} d={band} />
                <path className={styles.avg} d={`M${line.join("L")}`} fill="none" />
              </g>
            );
          })}

          {hovered !== undefined && (
            <g className={styles.crosshair}>
              <line
                className={styles.hair}
                x1={x(hovered.minute)}
                x2={x(hovered.minute)}
                y1={PAD_TOP}
                y2={PAD_TOP + PLOT_HEIGHT}
              />
              {hoveredPeak !== null && (
                <circle
                  className={styles.peakDot}
                  cx={x(hovered.minute)}
                  cy={y(hoveredPeak)}
                  r={3}
                />
              )}
              {hoveredAvg !== null && (
                <circle className={styles.dot} cx={x(hovered.minute)} cy={y(hoveredAvg)} r={3} />
              )}
            </g>
          )}
        </svg>

        {hover !== null && hovered !== undefined && (
          <div
            className={`${styles.readout} ${hover.left > (measured || width) / 2 ? styles.readoutLeft : ""}`}
            style={{ left: `${hover.left}px` }}
          >
            <div className={styles.readoutTime}>{clock.format(hovered.minute)}</div>
            <div className={styles.readoutRow}>
              <span className={styles.keyBand} />
              <strong>{hoveredPeak === null ? "—" : unit.value(hoveredPeak)}</strong>
              <span>{t("mixengine.metrics.peak")}</span>
            </div>
            <div className={styles.readoutRow}>
              <span className={styles.keyLine} />
              <strong>{hoveredAvg === null ? "—" : unit.value(hoveredAvg)}</strong>
              <span>{t("mixengine.metrics.average")}</span>
            </div>
            <div className={styles.readoutSamples}>
              {t("mixengine.metrics.samples", { count: hovered.samples })}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
