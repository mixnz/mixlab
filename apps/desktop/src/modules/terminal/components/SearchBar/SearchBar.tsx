import { useEffect, useRef, useState } from "react";
import Button from "../../../../components/Button";
import Input from "../../../../components/Input";
import { ChevronDownIcon, ChevronUpIcon, CloseIcon } from "../../../../icons";
import { useTranslation } from "../../../../i18n";
import styles from "./SearchBar.module.css";

interface Props {
  /** Searches for `query`, forwards (`back` false) or backwards (`back` true). Returns `false` when
   *  there is no match — the bar says so instead of staying silent as if nothing had been typed. */
  onFind: (query: string, back: boolean) => boolean;
  onClose: () => void;
  /** Bumped by one each time `Ctrl+F` is pressed. With the bar already open that key cannot open
   *  anything again — what it has to do is pull the keyboard back to the field and preselect its
   *  contents, just like a browser's find bar. A number rather than a flag: the same gesture
   *  repeated has to trigger again. */
  focusSignal: number;
}

/** The find bar over the scrolled-back part of a terminal session. */
function SearchBar({ onFind, onClose, focusSignal }: Props) {
  const { t } = useTranslation();
  const inputRef = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState("");
  /* `null` means no search yet — quite different from "searched and found nothing", and only the
     latter is worth saying. */
  const [found, setFound] = useState<boolean | null>(null);

  /* A bar that just appeared is there to be typed into; a bar opening while the keyboard stays
     elsewhere is pointless. And `Ctrl+F` pressed again while the bar is open reruns exactly this
     effect — `select()` is what gives the second press meaning: type over the old string to search
     for something else. */
  useEffect(() => {
    const input = inputRef.current;
    if (!input) return;
    input.focus();
    input.select();
  }, [focusSignal]);

  function find(text: string, back: boolean) {
    setQuery(text);
    if (text === "") {
      setFound(null);
      return;
    }
    setFound(onFind(text, back));
  }

  return (
    <div className={styles.bar}>
      <Input
        ref={inputRef}
        size="small"
        className={styles.field}
        placeholder={t("terminal.findPlaceholder")}
        aria-label={t("terminal.findPlaceholder")}
        value={query}
        /* Searches on every key: the result jumps along with what is typed, just like a browser's
           find bar. `back` is always false here — typing one more letter narrows forwards, not
           backwards. */
        onChange={(e) => find(e.target.value, false)}
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            e.preventDefault();
            onClose();
            return;
          }
          if (e.key !== "Enter") return;
          e.preventDefault();
          find(query, e.shiftKey);
        }}
      />
      {found === false && <span className={styles.status}>{t("terminal.findNoMatch")}</span>}
      <Button
        size="small"
        disabled={query === ""}
        title={t("terminal.findPrevious")}
        aria-label={t("terminal.findPrevious")}
        onClick={() => find(query, true)}
      >
        <ChevronUpIcon size="0.9em" />
      </Button>
      <Button
        size="small"
        disabled={query === ""}
        title={t("terminal.findNext")}
        aria-label={t("terminal.findNext")}
        onClick={() => find(query, false)}
      >
        <ChevronDownIcon size="0.9em" />
      </Button>
      <Button
        size="small"
        title={t("terminal.findClose")}
        aria-label={t("terminal.findClose")}
        onClick={onClose}
      >
        <CloseIcon size="0.9em" />
      </Button>
    </div>
  );
}

export default SearchBar;
