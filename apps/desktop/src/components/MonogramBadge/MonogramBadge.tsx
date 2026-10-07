import { monogramHue, monogramLetters } from "./monogram";
import styles from "./MonogramBadge.module.css";

export type MonogramSize = 28 | 30 | 34 | 38 | 50;

interface Props {
  /** The name the letters and the hue are taken from — a package, a service, a blueprint. */
  name: string;
  size?: MonogramSize;
  className?: string;
  /** A few characters pinned to the corner — the version that tells two instances of one package
   *  apart when their letters and hue are the same. */
  tag?: string;
}

/** Two letters in a tinted square: a name made recognisable at a glance in a list or a table.
 *  Decorative — the name it stands for is always written beside it — so it is hidden from
 *  assistive technology. */
function MonogramBadge({ name, size = 30, className, tag }: Props) {
  const hue = monogramHue(name);
  return (
    <span
      aria-hidden="true"
      className={`${styles.badge} ${styles[`size${size}`]} ${hue === null ? styles.neutral : styles[hue]}${
        className ? ` ${className}` : ""
      }`}
    >
      {monogramLetters(name)}
      {tag !== undefined && tag !== "" && <span className={styles.tag}>{tag}</span>}
    </span>
  );
}

export default MonogramBadge;
