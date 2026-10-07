import styles from "./CpuRing.module.css";

/** The smallest arc drawn for a reading above zero, so a process that is working never looks idle. */
const MIN_VISIBLE = 2;

/**
 * A small ring that fills with a CPU share — beside the number, never instead of it, so it is
 * decorative and hidden from assistive technology. `null` (not measured) draws the track alone.
 */
export default function CpuRing({ share, size = 16 }: { share: number | null; size?: 16 | 18 }) {
  const stroke = size === 18 ? 2.5 : 2.4;
  const radius = size / 2 - stroke / 2 - 0.5;
  const circumference = 2 * Math.PI * radius;
  const clamped = share === null ? 0 : Math.min(100, share > 0 ? Math.max(MIN_VISIBLE, share) : 0);
  const centre = size / 2;
  return (
    <svg className={styles.ring} width={size} height={size} viewBox={`0 0 ${size} ${size}`} aria-hidden="true">
      <circle className={styles.track} cx={centre} cy={centre} r={radius} strokeWidth={stroke} />
      {clamped > 0 && (
        <circle
          className={styles.arc}
          cx={centre}
          cy={centre}
          r={radius}
          strokeWidth={stroke}
          strokeDasharray={`${(clamped / 100) * circumference} ${circumference}`}
          transform={`rotate(-90 ${centre} ${centre})`}
        />
      )}
    </svg>
  );
}
