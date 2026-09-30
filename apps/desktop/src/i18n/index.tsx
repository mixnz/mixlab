import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { onPreferencesChanged } from "../core/preferences";
import { LANGUAGE_KEY as STORAGE_KEY } from "../shell/storageKeys";
import { EN, VI } from "./dicts";

/** Every module's strings and the shared ones, as one object — see {@link ./dicts}. */
export type TranslationDict = typeof EN;
export type Language = "en" | "vi";

const DICTS: Record<Language, TranslationDict> = { en: EN, vi: VI };

type DotPaths<T, Prefix extends string = ""> = {
  [K in keyof T & string]: T[K] extends string
    ? `${Prefix}${K}`
    : DotPaths<T[K], `${Prefix}${K}.`>;
}[keyof T & string];

export type TranslationKey = DotPaths<TranslationDict>;

function readStoredLanguage(): Language {
  const stored = localStorage.getItem(STORAGE_KEY);
  return stored === "en" || stored === "vi" ? stored : "en";
}

export function resolve(dict: TranslationDict, key: TranslationKey): string {
  // key is always a string at compile time, but a Record<SomeUnion, TranslationKey> indexed by a
  // value read from disk (not a literal) can return undefined at runtime — see
  // docs/specs/2026-09-05-unknown-db-kind-crash-and-error-logging-design.md.
  if (typeof key !== "string") return String(key);
  const value = key.split(".").reduce<unknown>((acc, part) => {
    if (acc && typeof acc === "object" && part in acc) return (acc as Record<string, unknown>)[part];
    return undefined;
  }, dict);
  return typeof value === "string" ? value : key;
}

function interpolate(template: string, vars?: Record<string, string | number>): string {
  if (!vars) return template;
  return template.replace(/\{\{(\w+)\}\}/g, (match, name) => (name in vars ? String(vars[name]) : match));
}

interface I18nContextValue {
  lang: Language;
  setLang: (lang: Language) => void;
  t: (key: TranslationKey, vars?: Record<string, string | number>) => string;
}

const I18nContext = createContext<I18nContextValue | null>(null);

export function I18nProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState<Language>(readStoredLanguage);
  useEffect(() => onPreferencesChanged(() => setLangState(readStoredLanguage())), []);

  /* The dictionary `t` reads, behind a ref so that `t` itself is one function for the life of the
     app rather than a new one per language.

     That is worth more than it looks. `t` is closed over by callbacks all over the app, and a `t`
     that changed identity was one that had to be listed as a dependency in every one of them —
     twenty places did not, and quietly went on holding the dictionary they were built with, so an
     error raised after the user switched language came out in the language before it. Stable, the
     question stops being askable: whoever is holding `t` is holding the current one.

     Nothing is lost on the rendering side. A switch changes `lang`, which rebuilds the context
     value below, which re-renders every consumer — each calling this same `t`, now reading the new
     dictionary. The one thing that does not follow by itself is a `useMemo` that has already
     turned words into a value; those list `lang` beside `t`, and that is what marks them out. */
  const dict = useRef(DICTS[lang]);
  dict.current = DICTS[lang];

  const t = useCallback<I18nContextValue["t"]>(
    (key, vars) => interpolate(resolve(dict.current, key), vars),
    [],
  );

  const setLang = useCallback((next: Language) => {
    localStorage.setItem(STORAGE_KEY, next);
    setLangState(next);
  }, []);

  const value = useMemo<I18nContextValue>(() => ({ lang, setLang, t }), [lang, setLang, t]);

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useTranslation() {
  const ctx = useContext(I18nContext);
  if (!ctx) throw new Error("useTranslation must be used within I18nProvider");
  return ctx;
}
