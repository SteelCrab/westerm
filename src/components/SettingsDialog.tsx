import { useEffect, useRef, useState } from "react";
import { ExternalLink, Minus, Plus, X } from "lucide-react";
import { LOCALE_NAMES, LOCALES, useI18n, type Locale } from "../i18n/index.tsx";
import { FONT_FAMILY_MAX, type Settings } from "../settings.ts";
import { MAX_FONT_SIZE, MIN_FONT_SIZE, type TerminalFont } from "../terminal/fonts.ts";
import type { ThemeChoice } from "../theme.ts";
import type { UpdateState } from "../useUpdate.ts";

interface SettingsDialogProps {
  settings: Settings;
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

/** Preferences: language, theme, terminal font, and updates. Changes apply at once. */
export function SettingsDialog(props: SettingsDialogProps) {
  const { settings, onChange, font, update, onClose } = props;
  const { t, choice, setLocale, followSystem } = useI18n();
  const dialog = useRef<HTMLDivElement>(null);
  const [checking, setChecking] = useState(false);
  const [checked, setChecked] = useState(false);
  // The font applies on Enter or when the field loses focus: each change
  // reloads every terminal, which should not happen per keystroke.
  const [fontDraft, setFontDraft] = useState(settings.fontFamily);
  const commitFont = () => {
    if (fontDraft !== settings.fontFamily) onChange({ fontFamily: fontDraft });
  };
  // However the dialog closes (Escape, ×, a click outside), keep what was typed.
  const latest = useRef({ fontDraft, saved: settings.fontFamily, onChange });
  latest.current = { fontDraft, saved: settings.fontFamily, onChange };
  useEffect(
    () => () => {
      const { fontDraft: draft, saved, onChange: save } = latest.current;
      if (draft !== saved) save({ fontFamily: draft });
    },
    [],
  );

  useEffect(() => {
    dialog.current?.querySelector<HTMLElement>("select, button, input")?.focus();
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
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

        <section className="settings-group" aria-labelledby="settings-general">
          <h3 id="settings-general">{t.settings.general}</h3>
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
        </section>

        <section className="settings-group" aria-labelledby="settings-terminal">
          <h3 id="settings-terminal">{t.settings.terminal}</h3>
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
            <label htmlFor="settings-font">{t.settings.fontFamily}</label>
            <input
              id="settings-font"
              type="text"
              value={fontDraft}
              placeholder={props.ghosttyFont}
              maxLength={FONT_FAMILY_MAX}
              spellCheck={false}
              autoComplete="off"
              onChange={(event) => setFontDraft(event.target.value)}
              onBlur={commitFont}
              onKeyDown={(event) => {
                if (event.key === "Enter" && !event.nativeEvent.isComposing) commitFont();
              }}
            />
            <p className="settings-hint">{t.settings.fontFamilyHint(props.ghosttyFont)}</p>
          </div>
        </section>

        {props.updates && (
          <section className="settings-group" aria-labelledby="settings-updates">
            <h3 id="settings-updates">{t.settings.updates}</h3>
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
          </section>
        )}

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
