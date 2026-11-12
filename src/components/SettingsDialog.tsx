import { useEffect, useRef, useState } from "react";
import {
  CircleArrowUp,
  ExternalLink,
  Minus,
  Palette,
  Plus,
  SlidersHorizontal,
  SquareTerminal,
  X,
  type LucideIcon,
} from "lucide-react";
import { LOCALE_NAMES, LOCALES, useI18n, type Locale } from "../i18n/index.tsx";
import { parseFamilies, type Layout, type Settings } from "../settings.ts";
import { MAX_FONT_SIZE, MIN_FONT_SIZE, type TerminalFont } from "../terminal/fonts.ts";
import { cssColor, COLOR_THEMES, terminalColors, themeName } from "../colorThemes.ts";
import type { ColorScheme, ThemeChoice } from "../theme.ts";
import type { UpdateState } from "../useUpdate.ts";
import { stepIndex } from "../uiHelpers.ts";
import { FontPicker } from "./FontPicker.tsx";

interface SettingsDialogProps {
  settings: Settings;
  /** The scheme in use, so the color themes preview the set the terminals show. */
  scheme: ColorScheme;
  onChange: (change: Partial<Settings>) => void;
  font: TerminalFont | null;
  ghosttyFont: string;
  sizeOverridden: boolean;
  changeSize: (delta: number) => void;
  resetSize: () => void;
  update: UpdateState;
  /** Opens the update panel in the header. */
  onShowUpdate: () => void;
  /** Shows the update section. False in a remote window: the server's westerm follows this one. */
  updates: boolean;
  onClose: () => void;
}

const REPOSITORY = "https://github.com/SteelCrab/westerm";

export type SettingsPage = "general" | "appearance" | "terminal" | "updates";

/** The pages of Settings, in menu order. A remote window has no updates page. */
export function settingsPages(updates: boolean): SettingsPage[] {
  const pages: SettingsPage[] = ["general", "appearance", "terminal"];
  return updates ? [...pages, "updates"] : pages;
}

const PAGE_ICONS: Record<SettingsPage, LucideIcon> = {
  general: SlidersHorizontal,
  appearance: Palette,
  terminal: SquareTerminal,
  updates: CircleArrowUp,
};

/**
 * Preferences, one page at a time from the menu: language and layout,
 * mode and color theme, terminal font, and updates. Changes apply at once.
 */
export function SettingsDialog(props: SettingsDialogProps) {
  const { settings, onChange, font, update, onClose } = props;
  const { t, choice, setLocale, followSystem } = useI18n();
  const dialog = useRef<HTMLDivElement>(null);
  const pages = settingsPages(props.updates);
  const [page, setPage] = useState<SettingsPage>("general");
  const [checking, setChecking] = useState(false);
  const [checked, setChecked] = useState(false);
  // Each font change reloads every terminal; the list changes one step at a time.
  const families = parseFamilies(settings.fontFamily);

  // On open only: `onClose` is a new function on each render, and a settings
  // change (a font added) must not move focus back to the menu.
  useEffect(() => {
    dialog.current?.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')?.focus();
  }, []);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      // A part with its own Escape (the font search) closes first.
      if (event.target instanceof Element && event.target.closest("[data-escape-own]")) return;
      event.preventDefault();
      event.stopPropagation();
      onClose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);

  const themes: Array<[ThemeChoice, string]> = [
    ["system", t.settings.themeSystem],
    ["light", t.settings.themeLight],
    ["dark", t.settings.themeDark],
  ];
  const layouts: Array<[Layout, string]> = [
    ["top", t.settings.layoutTop],
    ["sidebar", t.settings.layoutSidebar],
  ];

  const titles: Record<SettingsPage, string> = {
    general: t.settings.general,
    appearance: t.settings.appearance,
    terminal: t.settings.terminal,
    updates: t.settings.updates,
  };
  // The menu is one tab stop; the arrow keys move between its pages.
  const onMenuKey = (event: React.KeyboardEvent) => {
    const keys: Record<string, 1 | -1> = { ArrowDown: 1, ArrowRight: 1, ArrowUp: -1, ArrowLeft: -1 };
    const delta = keys[event.key];
    if (!delta) return;
    event.preventDefault();
    const next = pages[stepIndex(pages.indexOf(page), delta, pages.length)];
    setPage(next);
    dialog.current?.querySelector<HTMLElement>(`#settings-tab-${next}`)?.focus();
  };

  const checkNow = async () => {
    setChecking(true);
    await update.check(true);
    setChecking(false);
    setChecked(true);
  };

  const status = update.status;
  let updateLine: React.ReactNode = null;
  if (checking) updateLine = t.settings.checking;
  else if (checked && (!status || (!status.latest && !status.disabled))) updateLine = t.update.checkFailed;
  else if (status?.disabled) updateLine = t.update.disabled;
  else if (status?.newer && status.latest) {
    updateLine = (
      <>
        {t.update.heading(status.latest)}{" "}
        <button type="button" className="link-button" onClick={props.onShowUpdate}>
          {t.update.install}
        </button>
      </>
    );
  } else if (checked && status) updateLine = t.update.upToDate(status.current);

  return (
    <div className="settings-backdrop" onMouseDown={onClose}>
      <div
        className="settings"
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-title"
        ref={dialog}
        onMouseDown={(event) => event.stopPropagation()}
        onKeyDown={(event) => event.stopPropagation()}
      >
        <header className="settings-head">
          <h2 id="settings-title">{t.settings.title}</h2>
          <button type="button" className="icon-button" aria-label={t.settings.close} onClick={onClose}>
            <X size={15} />
          </button>
        </header>

        <div className="settings-body">
          <nav className="settings-nav" role="tablist" aria-orientation="vertical" aria-label={t.settings.title} onKeyDown={onMenuKey}>
            {pages.map((id) => {
              const Icon = PAGE_ICONS[id];
              return (
                <button
                  key={id}
                  id={`settings-tab-${id}`}
                  type="button"
                  role="tab"
                  aria-selected={page === id}
                  aria-controls="settings-panel"
                  tabIndex={page === id ? 0 : -1}
                  onClick={() => setPage(id)}
                >
                  <Icon size={15} aria-hidden="true" />
                  {titles[id]}
                </button>
              );
            })}
          </nav>

          <section id="settings-panel" className="settings-panel" role="tabpanel" aria-labelledby={`settings-tab-${page}`}>
            <h3>{titles[page]}</h3>

            {page === "general" && (
              <>
                <div className="settings-row">
                  <label htmlFor="settings-language">{t.settings.language}</label>
                  <select
                    id="settings-language"
                    value={choice}
                    onChange={(event) => {
                      const value = event.target.value;
                      if (value === "system") followSystem();
                      else setLocale(value as Locale);
                    }}
                  >
                    <option value="system">{t.settings.systemLanguage}</option>
                    {LOCALES.map((locale) => (
                      <option key={locale} value={locale}>
                        {LOCALE_NAMES[locale]}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="settings-row">
                  <span id="settings-layout-label">{t.settings.layout}</span>
                  <div className="seg" role="radiogroup" aria-labelledby="settings-layout-label">
                    {layouts.map(([value, label]) => (
                      <button
                        key={value}
                        type="button"
                        role="radio"
                        aria-checked={settings.layout === value}
                        onClick={() => onChange({ layout: value })}
                      >
                        {label}
                      </button>
                    ))}
                  </div>
                </div>
              </>
            )}

            {page === "appearance" && (
              <>
                <div className="settings-row">
                  <span id="settings-theme-label">{t.settings.theme}</span>
                  <div className="seg" role="radiogroup" aria-labelledby="settings-theme-label">
                    {themes.map(([value, label]) => (
                      <button
                        key={value}
                        type="button"
                        role="radio"
                        aria-checked={settings.theme === value}
                        onClick={() => onChange({ theme: value })}
                      >
                        {label}
                      </button>
                    ))}
                  </div>
                </div>
                <div className="settings-row stacked">
                  <span id="settings-color-theme-label">{t.settings.colorTheme}</span>
                  <div className="color-themes" role="radiogroup" aria-labelledby="settings-color-theme-label">
                    {COLOR_THEMES.map((id) => {
                      const colors = terminalColors(id, props.scheme);
                      return (
                        <button
                          key={id}
                          type="button"
                          role="radio"
                          aria-checked={settings.colorTheme === id}
                          className="color-theme"
                          onClick={() => onChange({ colorTheme: id })}
                        >
                          <span
                            className="color-theme-preview"
                            style={{ background: cssColor(colors.background), color: cssColor(colors.foreground) }}
                            aria-hidden="true"
                          >
                            <span className="color-theme-text" />
                            <span className="color-theme-swatches">
                              {colors.palette.slice(1, 7).map((color, index) => (
                                <span key={index} style={{ background: cssColor(color) }} />
                              ))}
                            </span>
                          </span>
                          <span className="color-theme-name">{themeName(id) ?? t.settings.colorThemeDefault}</span>
                        </button>
                      );
                    })}
                  </div>
                  <p className="settings-hint">{t.settings.colorThemeHint}</p>
                </div>
              </>
            )}

            {page === "terminal" && (
              <>
                <div className="settings-row">
                  <span id="settings-size-label">{t.settings.fontSize}</span>
                  <div className="settings-stepper" role="group" aria-labelledby="settings-size-label">
                    <button
                      type="button"
                      className="icon-button"
                      aria-label={t.font.smaller}
                      disabled={!font || font.size <= MIN_FONT_SIZE}
                      onClick={() => props.changeSize(-1)}
                    >
                      <Minus size={13} />
                    </button>
                    <output aria-live="polite">{font ? `${font.size}px` : "…"}</output>
                    <button
                      type="button"
                      className="icon-button"
                      aria-label={t.font.bigger}
                      disabled={!font || font.size >= MAX_FONT_SIZE}
                      onClick={() => props.changeSize(1)}
                    >
                      <Plus size={13} />
                    </button>
                    <button type="button" className="link-button" disabled={!props.sizeOverridden} onClick={props.resetSize}>
                      {t.settings.fontSizeReset}
                    </button>
                  </div>
                </div>
                <div className="settings-row stacked">
                  <div className="settings-row">
                    <span>{t.settings.fontFamily}</span>
                    <button
                      type="button"
                      className="link-button"
                      disabled={families.length === 0}
                      onClick={() => onChange({ fontFamily: "" })}
                    >
                      {t.fontPicker.reset}
                    </button>
                  </div>
                  <FontPicker
                    families={families}
                    ghosttyFont={props.ghosttyFont}
                    onChange={(next) => onChange({ fontFamily: next.join(", ") })}
                  />
                  <p className="settings-hint">{t.settings.fontFamilyHint(props.ghosttyFont)}</p>
                  {font && (
                    <div className="font-preview" style={{ fontFamily: font.stack, fontSize: font.size }} aria-hidden>
                      <span className="font-preview-dir">{"\u{f07c} ~/westerm"}</span>{" "}
                      <span className="font-preview-branch">{"\u{e0a0} main"}</span> ❯ ls -la{"\n"}
                      한글 日本語 中文 0O 1lI {"{} => != ->"}
                    </div>
                  )}
                </div>
                <div className="settings-row stacked">
                  <label className="settings-check" htmlFor="settings-resume-agents">
                    <input
                      id="settings-resume-agents"
                      type="checkbox"
                      checked={settings.resumeAgents}
                      onChange={(event) => onChange({ resumeAgents: event.target.checked })}
                    />
                    {t.settings.resumeAgents}
                  </label>
                  <p className="settings-hint">{t.settings.resumeAgentsHint}</p>
                </div>
              </>
            )}

            {page === "updates" && (
              <>
                <div className="settings-row stacked">
                  <label className="settings-check" htmlFor="settings-auto-update">
                    <input
                      id="settings-auto-update"
                      type="checkbox"
                      checked={settings.autoUpdate}
                      onChange={(event) => onChange({ autoUpdate: event.target.checked })}
                    />
                    {t.settings.autoUpdate}
                  </label>
                  <p className="settings-hint">{t.settings.autoUpdateHint}</p>
                </div>
                <div className="settings-row">
                  <span className="settings-version">{t.settings.version(status?.current ?? "…")}</span>
                  <button type="button" className="button" disabled={checking} onClick={checkNow}>
                    {t.settings.checkNow}
                  </button>
                </div>
                {updateLine && (
                  <p className="settings-hint" role="status">
                    {updateLine}
                  </p>
                )}
              </>
            )}
          </section>
        </div>

        <footer className="settings-foot">
          <a href={REPOSITORY} target="_blank" rel="noreferrer">
            {t.settings.sourceCode} <ExternalLink size={12} aria-hidden="true" />
          </a>
          <a href={`${REPOSITORY}/blob/main/LICENSE`} target="_blank" rel="noreferrer">
            {t.settings.license} <ExternalLink size={12} aria-hidden="true" />
          </a>
          <span className="settings-hint">{t.settings.savedHere}</span>
        </footer>
      </div>
    </div>
  );
}
