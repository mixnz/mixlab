import { useId } from "react";

import Switch from "../Switch";
import styles from "./SwitchTile.module.css";

interface Props {
  label: string;
  checked: boolean;
  onChange: (next: boolean) => void;
  disabled?: boolean;
  /** The label in the monospace face — for a name the user types or reads as code. */
  mono?: boolean;
  /** `data-demo` for the promotional clips (`demo/clips.mjs`). Inert everywhere else. */
  demo?: string;
}

/**
 * A name and its small switch in one bordered tile, for a grid of things each on or off — the PHP
 * extensions of a version, the services of a site.
 *
 * The whole tile is the switch's label, so a click anywhere on it flips it; off, it sinks into the
 * surface and its name fades.
 */
function SwitchTile({ label, checked, onChange, disabled, mono, demo }: Props) {
  const id = useId();
  const classes = [styles.tile];
  if (!checked) classes.push(styles.off);
  if (disabled) classes.push(styles.disabled);
  return (
    <label htmlFor={id} className={classes.join(" ")} data-demo={demo}>
      <span className={mono ? `${styles.name} ${styles.mono}` : styles.name}>{label}</span>
      <Switch small id={id} checked={checked} disabled={disabled} onChange={onChange} />
    </label>
  );
}

export default SwitchTile;
