import LoadingState from "../../../../components/LoadingState";
import styles from "./Settings.module.css";

/** A section whose first read has not answered yet: its title, and the wait under it.
 *
 * Drawn in place of `return null`, so each section holds its place on the page while it reads
 * instead of popping in, in whatever order the answers arrive, and pushing the rest down. */
export default function SectionLoading({ title }: { title: string }) {
  return (
    <section className={styles.section}>
      <h3 className={styles.sectionTitle}>{title}</h3>
      <LoadingState compact />
    </section>
  );
}
