import { useMemo, useState } from "react";
import Button from "../../../../components/Button";
import Input, { Textarea } from "../../../../components/Input";
import Select, { type SelectOption } from "../../../../components/Select";
import { useTranslation } from "../../../../i18n";
import CopyField from "../../components/CopyField";
import { BUILTIN } from "./builtin";
import {
  addSnippet,
  fill,
  paramsOf,
  removeSnippet,
  updateSnippet,
  type SnippetDraft,
} from "./snippets";
import { saveSnippets, useSnippets } from "./snippetsStore";
import styles from "./Panel.module.css";

const EMPTY_DRAFT: SnippetDraft = { title: "", group: "", template: "" };

function CheatsheetPanel() {
  const { t } = useTranslation();
  const mine = useSnippets();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [values, setValues] = useState<Record<string, string>>({});
  /** `null` when nothing is being edited. `id` is `null` while adding a new one. */
  const [draft, setDraft] = useState<{ id: string | null; fields: SnippetDraft } | null>(null);

  const all = useMemo(() => [...BUILTIN, ...mine], [mine]);
  const selected = all.find((snippet) => snippet.id === selectedId) ?? null;
  const isMine = selected !== null && mine.some((snippet) => snippet.id === selected.id);

  const options: SelectOption<string>[] = all.map((snippet) => ({
    value: snippet.id,
    label: `${snippet.group} · ${snippet.title}`,
  }));

  const params = selected ? paramsOf(selected.template) : [];
  const filled = selected ? fill(selected.template, values) : "";

  const pick = (id: string): void => {
    setSelectedId(id);
    // The previous command's values mean nothing to the next command.
    setValues({});
    setDraft(null);
  };

  const commit = (): void => {
    if (!draft) return;
    const next =
      draft.id === null
        ? addSnippet(mine, draft.fields)
        : updateSnippet(mine, draft.id, draft.fields);
    saveSnippets(next);
    setDraft(null);
    if (draft.id === null) {
      // The item just added is the last one; select it right away so the user can use it at once.
      setSelectedId(next[next.length - 1]?.id ?? null);
      setValues({});
    }
  };

  const drop = (): void => {
    if (!selected) return;
    saveSnippets(removeSnippet(mine, selected.id));
    setSelectedId(null);
    setValues({});
  };

  return (
    <div className={styles.panel}>
      <div className={styles.controls}>
        <Select
          value={selectedId ?? ""}
          options={options}
          onChange={pick}
          ariaLabel={t("toolbox.cheatsheet.snippet")}
          placeholder={t("toolbox.cheatsheet.empty")}
          className={styles.snippet}
          searchable
        />
        <Button onClick={() => setDraft({ id: null, fields: EMPTY_DRAFT })}>
          {t("toolbox.cheatsheet.add")}
        </Button>
        {/* Built-in snippets cannot be edited or deleted — see `builtin.ts`. */}
        <Button
          onClick={() => selected && setDraft({ id: selected.id, fields: { ...selected } })}
          disabled={!isMine}
        >
          {t("toolbox.cheatsheet.edit")}
        </Button>
        <Button onClick={drop} disabled={!isMine}>
          {t("toolbox.cheatsheet.remove")}
        </Button>
      </div>

      {draft ? (
        <div className={styles.form}>
          <div className={styles.formRow}>
            <Input
              value={draft.fields.title}
              onChange={(event) =>
                setDraft({ ...draft, fields: { ...draft.fields, title: event.target.value } })
              }
              placeholder={t("toolbox.cheatsheet.title")}
              aria-label={t("toolbox.cheatsheet.title")}
            />
            <Input
              value={draft.fields.group}
              onChange={(event) =>
                setDraft({ ...draft, fields: { ...draft.fields, group: event.target.value } })
              }
              placeholder={t("toolbox.cheatsheet.group")}
              aria-label={t("toolbox.cheatsheet.group")}
            />
          </div>
          <Textarea
            value={draft.fields.template}
            onChange={(event) =>
              setDraft({ ...draft, fields: { ...draft.fields, template: event.target.value } })
            }
            placeholder={t("toolbox.cheatsheet.template")}
            aria-label={t("toolbox.cheatsheet.template")}
            maxRows={6}
          />
          <div className={styles.controls}>
            <Button
              variant="primary"
              onClick={commit}
              disabled={draft.fields.title.trim() === "" || draft.fields.template.trim() === ""}
            >
              {t("toolbox.cheatsheet.save")}
            </Button>
            <Button onClick={() => setDraft(null)}>{t("toolbox.cheatsheet.cancel")}</Button>
          </div>
        </div>
      ) : null}

      {selected ? (
        <>
          {params.length === 0 ? (
            <p className={styles.note}>{t("toolbox.cheatsheet.noParams")}</p>
          ) : (
            <div className={styles.params}>
              {params.map((name) => (
                <label key={name} className={styles.field}>
                  <span className={styles.fieldLabel}>{name}</span>
                  <Input
                    value={values[name] ?? ""}
                    onChange={(event) => setValues({ ...values, [name]: event.target.value })}
                  />
                </label>
              ))}
            </div>
          )}
          <CopyField label={t("toolbox.cheatsheet.result")} value={filled} multiline />
          <p className={styles.note}>{t("toolbox.cheatsheet.note")}</p>
        </>
      ) : null}
    </div>
  );
}

export default CheatsheetPanel;
