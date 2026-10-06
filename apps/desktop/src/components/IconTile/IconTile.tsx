import type { ReactNode } from "react";

import type { MonogramSize } from "../MonogramBadge";
import styles from "./IconTile.module.css";

/** The same scale as `MonogramBadge`, so an icon tile and a monogram in one table are one size. */
export type IconTileSize = MonogramSize;

/** What the tile's wash says: a state (`success`, `danger`, `neutral`) or a category (`coral`). */
export type IconTileTone = "success" | "danger" | "neutral" | "coral";

interface Props {
  /** One icon from `src/icons`, at the size the tile is drawn for. */
  children: ReactNode;
  size?: IconTileSize;
  tone?: IconTileTone;
  className?: string;
}

/**
 * An icon in a tinted square — `MonogramBadge`'s shape for a thing that has an icon rather than a
 * name: a project's folder, a site's lock, a domain's globe.
 *
 * **One component because three screens drew it by hand** and drifted to three sizes in tables of
 * one row height. Decorative, like the badge: the name it stands beside says what the row is, so
 * it is hidden from assistive technology.
 */
function IconTile({ children, size = 34, tone = "neutral", className }: Props) {
  return (
    <span
      aria-hidden="true"
      className={`${styles.tile} ${styles[`size${size}`]} ${styles[tone]}${className ? ` ${className}` : ""}`}
    >
      {children}
    </span>
  );
}

export default IconTile;
