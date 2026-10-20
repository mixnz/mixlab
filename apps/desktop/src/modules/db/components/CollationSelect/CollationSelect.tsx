import { useMemo } from "react";
import Input from "../../../../components/Input";
import Select from "../../../../components/Select";
import type { SelectOption } from "../../../../components/Select";
import { useLanguageT, useTranslation } from "../../../../i18n";
import type { SqlCollation } from "../../types";
import styles from "./CollationSelect.module.css";

/** The character sets a column or table is realistically declared in, most likely first. Everything
 * else the server offers follows them, alphabetically — the order is only about what is quick to
 * reach. */
const CHARSET_ORDER = ["utf8mb4", "utf8mb3", "utf8", "latin1", "ascii", "binary"];

function charsetRank(charset: string): number {
  const index = CHARSET_ORDER.indexOf(charset);
  return index === -1 ? CHARSET_ORDER.length : index;
}

interface Props {
  /** The collation currently chosen, or `""` for none — which means inheriting a default. */
  value: string;
  /** What this server supports. Empty — a server that would not say, or a list still on its way —
   *  leaves this a text box, which is what it was before there was a list. */
  collations: SqlCollation[];
  /** What the empty choice reads as: whose default is inherited by leaving this alone. */
  placeholder: string;
  ariaLabel: string;
  disabled?: boolean;
  onChange: (collation: string) => void;
}

/**
 * The collation picker, shared by everything that declares one. The list belongs to the server, so
 * it is read once per connection and handed down rather than fetched here.
 */
function CollationSelect({ value, collations, placeholder, ariaLabel, disabled, onChange }: Props) {
  const { t } = useTranslation();
  // The options are words made once per language: a new function per language rebuilds them.
  const tl = useLanguageT();
  const current = value.trim();

  /** The list as it is offered: the chosen collation's own character set first, then the ones most
   * things use, and inside each the character set's default ahead of the rest. */
  const options = useMemo(() => {
    // Changing a collation nearly always means changing it within the character set already in
    // use, so that set's collations sit above every other.
    const currentCharset = collations.find((c) => c.name === current)?.charset;
    const sorted = [...collations].sort((a, b) => {
      if (a.charset !== b.charset) {
        if (a.charset === currentCharset) return -1;
        if (b.charset === currentCharset) return 1;
        return charsetRank(a.charset) - charsetRank(b.charset) || a.charset.localeCompare(b.charset);
      }
      if (a.isDefault !== b.isDefault) return a.isDefault ? -1 : 1;
      return a.name.localeCompare(b.name);
    });

    const list: SelectOption<string>[] = [
      { value: "", label: placeholder },
      ...sorted.map((collation) => ({
        value: collation.name,
        label: collation.name,
        optionLabel: (
          <span className={styles.option}>
            <span>{collation.name}</span>
            <span className={styles.charset}>
              {collation.charset === ""
                ? tl("collation.anyCharset")
                : collation.isDefault
                  ? tl("collation.charsetDefault", { charset: collation.charset })
                  : collation.charset}
            </span>
          </span>
        ),
        // The charset is searchable too: typing "latin1" is how its collations are found.
        searchText: `${collation.name} ${collation.charset}`,
      })),
    ];
    // A collation this server no longer lists — an old column, a character set since dropped —
    // would otherwise leave the trigger blank and be lost the moment it is saved.
    if (current !== "" && !collations.some((c) => c.name === current)) {
      list.splice(1, 0, { value: current, label: current });
    }
    return list;
  }, [collations, current, placeholder, tl]);

  if (collations.length === 0) {
    return (
      <Input
        size="normal"
        value={value}
        placeholder={placeholder}
        aria-label={ariaLabel}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value)}
      />
    );
  }

  return (
    <Select
      value={current}
      size="normal"
      options={options}
      ariaLabel={ariaLabel}
      disabled={disabled}
      searchable
      searchPlaceholder={t("collation.search")}
      onChange={onChange}
    />
  );
}

export default CollationSelect;
