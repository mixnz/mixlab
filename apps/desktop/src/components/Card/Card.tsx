import type { ReactNode } from "react";
import styles from "./Card.module.css";

interface Props {
  title?: ReactNode;
  description?: ReactNode;
  /** A figure after the title, in the muted colour — "3 runtimes", "4 of 31 on". */
  count?: ReactNode;
  /** Controls at the right end of the header. */
  actions?: ReactNode;
  /** No padding around the body, for a table that runs edge to edge under the header. */
  flush?: boolean;
  /** The heading element — a card on a page is an `h2`, one inside a detail pane an `h3`. */
  headingLevel?: 2 | 3;
  /** Tone of the whole card; `danger` for the one that holds an irreversible action. */
  tone?: "default" | "danger";
  className?: string;
  /** `data-demo-focus` on the card: a block the promotional clips' camera frames
   *  (`demo/focus.mjs`). Inert everywhere else. */
  demoFocus?: string;
  children?: ReactNode;
}

/** A section of a screen: a surface, a hairline, and an optional header naming what is inside. */
function Card({
  title,
  description,
  count,
  actions,
  flush,
  headingLevel = 2,
  tone = "default",
  className,
  demoFocus,
  children,
}: Props) {
  const Heading = headingLevel === 2 ? "h2" : "h3";
  const hasHeader = title !== undefined || description !== undefined || actions !== undefined;
  return (
    <section
      className={`${styles.card}${flush ? ` ${styles.flush}` : ""}${tone === "danger" ? ` ${styles.danger}` : ""}${
        className ? ` ${className}` : ""
      }`}
      data-demo-focus={demoFocus}
    >
      {hasHeader && (
        <div className={styles.header}>
          <div className={styles.heading}>
            {title !== undefined && (
              <div className={styles.titleRow}>
                <Heading className={styles.title}>{title}</Heading>
                {count !== undefined && <span className={styles.count}>{count}</span>}
              </div>
            )}
            {description !== undefined && <p className={styles.description}>{description}</p>}
          </div>
          {actions !== undefined && <div className={styles.actions}>{actions}</div>}
        </div>
      )}
      {children !== undefined && <div className={styles.body}>{children}</div>}
    </section>
  );
}

export default Card;
