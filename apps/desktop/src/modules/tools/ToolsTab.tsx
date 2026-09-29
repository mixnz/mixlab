import { useEffect, useState } from "react";
import type { ModuleTabProps } from "../../shell/module";
import { useTranslation } from "../../i18n";
import Input from "../../components/Input";
import ToolList from "./components/ToolList";
import { TOOLS } from "./registry";
import { parseToolsTabState } from "./tabState";
import { clearToolUse, loadFrequentTools, recordToolUse } from "./usageStore";
import "./tools.css";

/** How many tools the "Frequently used" group holds at most. */
const FREQUENT_LIMIT = 5;

function ToolsTab({ onTitleChange, onStateChange, restored }: ModuleTabProps) {
  const { t, lang } = useTranslation();
  const [query, setQuery] = useState("");

  // Order is a snapshot taken once per mount — opening a Tools tab, or reloading the app, is the
  // only time ranking-by-use-count runs. Within one tab's lifetime, picking tools must not reshuffle
  // rows the user is looking at; `pick` below only ever appends to the end of this list.
  const [frequentIds, setFrequentIds] = useState<string[]>([]);
  useEffect(() => {
    let cancelled = false;
    loadFrequentTools(FREQUENT_LIMIT).then((ids) => {
      if (!cancelled) setFrequentIds(ids);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  // Read `restored` exactly once, here. Reading it reactively would make the module overwrite
  // itself on its very first write — the rule lives in `shell/module.ts`.
  // No tool by default — only a saved pick from a previous run reopens one. A fresh tab lands on
  // the empty prompt instead of steering everyone toward whichever tool sits first in the registry.
  const [selectedId, setSelectedId] = useState<string | null>(() => {
    const saved = parseToolsTabState(restored);
    if (saved && TOOLS.some((tool) => tool.id === saved.toolId)) return saved.toolId;
    return null;
  });

  const selected = TOOLS.find((tool) => tool.id === selectedId) ?? null;

  // The tab title is the tool's name, not "Tools": three Tools tabs open at once can be told apart.
  // `onTitleChange` is not in the deps — the shell returns a new closure on every render, and
  // `shell/tabs.ts` names the loop that listing it would create.
  // `lang` next to `t`: `t` is a single function for the app's whole lifetime, so `lang` is what
  // signals that the text has changed and the title needs rebuilding. See `i18n/index.tsx`.
  useEffect(() => {
    onTitleChange(selected ? t(selected.labelKey) : t("toolbox.newTabTitle"));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selected, t, lang]);

  const pick = (id: string) => {
    setSelectedId(id);
    onStateChange({ toolId: id });
    recordToolUse(id);
    // A tool already shown keeps its row — only a tool newly entering the ranking is added, and
    // always at the bottom, never reordering what's already there. See the comment on `frequentIds`.
    // At capacity, the current bottom row (the lowest-ranked one) makes way for it.
    setFrequentIds((ids) => {
      if (ids.includes(id)) return ids;
      const room = ids.length >= FREQUENT_LIMIT ? ids.slice(0, -1) : ids;
      return [...room, id];
    });
  };

  const removeFrequent = (id: string) => {
    clearToolUse(id);
    setFrequentIds((ids) => ids.filter((toolId) => toolId !== id));
  };

  return (
    <div className="tools-tab">
      <aside className="tools-sidebar">
        <Input
          size="normal"
          allowClear
          className="tools-sidebar-search"
          placeholder={t("toolbox.searchPlaceholder")}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <div className="tools-sidebar-list">
          <ToolList
            tools={TOOLS}
            selectedId={selectedId}
            onSelect={pick}
            query={query}
            frequentIds={frequentIds}
            onRemoveFrequent={removeFrequent}
          />
        </div>
      </aside>
      <div className="tools-pane">
        {selected ? <selected.Panel /> : <p className="muted">{t("toolbox.selectPrompt")}</p>}
      </div>
    </div>
  );
}

export default ToolsTab;
