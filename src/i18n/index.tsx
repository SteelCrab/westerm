import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { detectLocale, HTML_LANG, MESSAGES, type Locale, type Messages } from "./messages.ts";

export * from "./messages.ts";

const KEY = "westerm.locale";

function readSaved(): string | null {
  try {
    return localStorage.getItem(KEY);
  } catch {
    return null;
  }
}

interface I18n {
  locale: Locale;
  /** "system" while the language follows the browser. */
  choice: Locale | "system";
  t: Messages;
  setLocale: (locale: Locale) => void;
  /** Forget the chosen language and follow the browser again. */
  followSystem: () => void;
}

const I18nContext = createContext<I18n | null>(null);

/** Language for the whole UI: the saved choice, else the browser's language. */
export function I18nProvider({ children }: { children: ReactNode }) {
  const [saved, setSaved] = useState<Locale | null>(() => {
    const raw = readSaved();
    return raw && detectLocale(raw, []) === raw ? (raw as Locale) : null;
  });
  const locale = saved ?? detectLocale(null, navigator.languages ?? []);

  useEffect(() => {
    document.documentElement.lang = HTML_LANG[locale];
  }, [locale]);

  const setLocale = useCallback((next: Locale) => {
    setSaved(next);
    try {
      localStorage.setItem(KEY, next);
    } catch {
      // The choice still applies to this page.
    }
  }, []);

  const followSystem = useCallback(() => {
    setSaved(null);
    try {
      localStorage.removeItem(KEY);
    } catch {
      // Nothing saved to forget.
    }
  }, []);

  const value = useMemo(
    () => ({ locale, choice: saved ?? ("system" as const), t: MESSAGES[locale], setLocale, followSystem }),
    [locale, saved, setLocale, followSystem],
  );
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18n {
  const value = useContext(I18nContext);
  if (!value) throw new Error("useI18n needs <I18nProvider>");
  return value;
}
