import { useEffect, useState } from "react";
import { CheckIcon, CopyIcon } from "../../../../icons";
import { useTranslation } from "../../../../i18n";
import { copyText } from "../../../../core/clipboard";
import styles from "./CopyField.module.css";

interface CopyFieldProps {
  label: string;
  value: string;
  /** Lets the field grow taller and scroll, for long multi-line values. */
  multiline?: boolean;
  /** Prints in a mono font. On by default — almost everything this module prints is code or ids. */
  mono?: boolean;
}

const COPIED_MS = 1500;

/**
 * One read-only result line with a copy button.
 *
 * The button reports the copy by changing itself for a second and a half. A failed copy is
 * swallowed here, just as the rest module's `RequestList` and `TreeView` do: `copyText` already
 * handles the hard part itself (it retries through the old `execCommand` path before giving up),
 * and a result line has nowhere to hang an error message.
 */
function CopyField({ label, value, multiline = false, mono = true }: CopyFieldProps) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), COPIED_MS);
    return () => clearTimeout(timer);
  }, [copied]);

  const copy = () => {
    void copyText(value)
      .then(() => setCopied(true))
      .catch(() => {});
  };

  return (
    <div className={styles.row}>
      <span className={styles.label}>{label}</span>
      <output
        className={`${styles.value}${mono ? ` ${styles.mono}` : ""}${multiline ? ` ${styles.multiline}` : ""}`}
      >
        {value}
      </output>
      <button
        type="button"
        className={styles.copy}
        onClick={copy}
        title={copied ? t("toolbox.copied") : t("toolbox.copy")}
        aria-label={copied ? t("toolbox.copied") : t("toolbox.copy")}
        disabled={value === ""}
      >
        {copied ? <CheckIcon /> : <CopyIcon />}
      </button>
    </div>
  );
}

export default CopyField;
