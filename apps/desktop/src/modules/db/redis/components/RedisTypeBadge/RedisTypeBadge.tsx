import type { RedisKeyType } from "../../api";
import styles from "./RedisTypeBadge.module.css";

/** The types with a colour of their own; anything else falls back to the neutral badge. */
const KNOWN_TYPES = new Set(["string", "list", "set", "zset", "hash", "stream"]);

/** What the badge says. Three characters, which is what the badge column fits — the colour is
 * what tells the types apart at a glance anyway, the letters only confirm it. */
const TYPE_ABBREVIATION: Record<string, string> = {
  string: "STR",
  list: "LST",
  set: "SET",
  zset: "ZST",
  hash: "HSH",
  stream: "STM",
};

interface Props {
  type: RedisKeyType;
  className?: string;
}

/**
 * A key's type as a fixed-width chip.
 *
 * Fixed width because every list that shows one puts it left of the key name, and the names line
 * up in one column only as long as the badge takes the same room whatever the type is. How wide
 * that is can be set from the outside with `--redis-badge-width`, for a list that has a second
 * thing to fit in the same column.
 */
function RedisTypeBadge({ type, className }: Props) {
  const typeClass = KNOWN_TYPES.has(type) ? ` ${styles[type]}` : "";
  return (
    <span className={`${styles.badge}${typeClass}${className ? ` ${className}` : ""}`}>
      {TYPE_ABBREVIATION[type] ?? type.slice(0, 3).toUpperCase()}
    </span>
  );
}

export default RedisTypeBadge;
