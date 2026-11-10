import { useEffect, useMemo, useRef, useState } from "react";
import { Check, GripVertical, Plus, Search, X } from "lucide-react";
import type { SystemFont } from "../../shared/protocol.ts";
import { afterMidpoint, carries, insertionIndex, startDrag } from "../dnd.ts";
import { useI18n, type Locale } from "../i18n/index.tsx";
import { MAX_FONT_FAMILIES } from "../settings.ts";
import { addFamily, fontTags, moveFamily, removeFamily, searchFonts } from "../terminal/fontList.ts";
import { cssFamily, isGenericFamily } from "../terminal/fonts.ts";
import { useSystemFonts } from "../useSystemFonts.ts";

const FONT_MIME = "application/x-westerm-font";

/** Letters of the UI language's script, shown for a font that has them. */
const SCRIPT_SAMPLE: Record<Locale, string> = { ko: "가나다", ja: "あいう", zh: "中文字", en: "가あ中" };
/** nf-fa-folder and nf-pl-branch, as prompts show them. */
const ICON_SAMPLE = "\u{f07b} \u{e0a0}";

/** A short line that shows what the font itself draws. */
function sampleText(font: SystemFont | undefined, locale: Locale): string {
  const tags = font ? fontTags(font, locale) : { script: false, icons: false };
  return ["Aa 0Ol1", tags.script ? SCRIPT_SAMPLE[locale] : "", tags.icons ? ICON_SAMPLE : ""].filter(Boolean).join(" ");
}

/** The family alone, then the UI font for letters it does not have (a symbols font). */
function ownFace(family: string): React.CSSProperties {
  return { fontFamily: `${cssFamily(family)}, var(--font-ui)` };
}

function sampleFace(family: string): React.CSSProperties {
  return { fontFamily: `${cssFamily(family)}, monospace` };
}

function Tags({ font, locale }: { font: SystemFont | undefined; locale: Locale }) {
  const { t } = useI18n();
  if (!font) return null;
  const tags = fontTags(font, locale);
  return (
    <>
      {tags.script && <span className="font-tag">{t.fontPicker.tagScript}</span>}
      {tags.icons && <span className="font-tag">{t.fontPicker.tagIcons}</span>}
    </>
  );
}

interface FontPickerProps {
  /** The chosen families, the first one first. Empty uses the Ghostty config. */
  families: string[];
  /** The Ghostty font, shown while the list is empty. */
  ghosttyFont: string;
  onChange: (families: string[]) => void;
}

/**
 * Settings ▸ Terminal ▸ Font: an ordered list of fonts, reordered by drag or
 * Alt+↑/↓, and a search over the fonts installed on this computer to add
 * more. Names that are not installed can be typed in.
 */
export function FontPicker({ families, ghosttyFont, onChange }: FontPickerProps) {
  const { t, locale } = useI18n();
  const installed = useSystemFonts();
  const fonts = useMemo(() => (installed.status === "ready" ? installed.fonts : []), [installed]);
  const byName = useMemo(() => new Map(fonts.map((font) => [font.family.toLowerCase(), font])), [fonts]);
  const [searching, setSearching] = useState(false);
  const [drop, setDrop] = useState<{ index: number; after: boolean } | null>(null);
  const dragFrom = useRef(-1);
  const rows = useRef(new Map<string, HTMLLIElement>());
  const addButton = useRef<HTMLButtonElement>(null);
  const full = families.length >= MAX_FONT_FAMILIES;
  // A keyboard move or removal puts focus here once the list has re-rendered.
  const [focusAfter, setFocusAfter] = useState<string | null>(null);

  useEffect(() => {
    if (focusAfter === null) return;
    (rows.current.get(focusAfter) ?? addButton.current)?.focus();
    setFocusAfter(null);
  }, [focusAfter, families]);

  const change = (next: string[]) => {
    if (next !== families) onChange(next);
  };

  const remove = (index: number) => {
    change(removeFamily(families, index));
    setFocusAfter(families[index + 1] ?? families[index - 1] ?? "");
  };

  const onRowKey = (event: React.KeyboardEvent, index: number) => {
    if (event.target !== event.currentTarget) return;
    if (event.altKey && (event.key === "ArrowUp" || event.key === "ArrowDown")) {
      event.preventDefault();
      change(moveFamily(families, index, index + (event.key === "ArrowUp" ? -1 : 1)));
      setFocusAfter(families[index]);
    } else if (event.key === "Delete" || event.key === "Backspace") {
      event.preventDefault();
      remove(index);
    }
  };

  const endDrag = () => {
    dragFrom.current = -1;
    setDrop(null);
  };

  return (
    <div className="font-picker">
      <ol className="font-stack" aria-label={t.fontPicker.listLabel}>
        {families.length === 0 && <li className="font-row empty">{t.fontPicker.empty(ghosttyFont)}</li>}
        {families.map((family, index) => {
          const font = byName.get(family.toLowerCase());
          const missing = installed.status === "ready" && !font && !isGenericFamily(family);
          const dropClass = drop?.index === index ? (drop.after ? " drop-after" : " drop-before") : "";
          return (
            <li
              key={family}
              ref={(element) => {
                if (element) rows.current.set(family, element);
                else rows.current.delete(family);
              }}
              className={`font-row${dropClass}`}
              tabIndex={0}
              draggable
              title={t.fontPicker.reorderHint}
              aria-keyshortcuts="Alt+ArrowUp Alt+ArrowDown Delete"
              onKeyDown={(event) => onRowKey(event, index)}
              onDragStart={(event) => {
                startDrag(event, FONT_MIME, family);
                dragFrom.current = index;
              }}
              onDragOver={(event) => {
                if (!carries(event, FONT_MIME)) return;
                event.preventDefault();
                const after = afterMidpoint(event, "y");
                if (drop?.index !== index || drop.after !== after) setDrop({ index, after });
              }}
              onDrop={(event) => {
                if (!carries(event, FONT_MIME)) return;
                event.preventDefault();
                const from = dragFrom.current;
                if (from >= 0) change(moveFamily(families, from, insertionIndex(from, index, afterMidpoint(event, "y"))));
                endDrag();
              }}
              onDragEnd={endDrag}
            >
              <GripVertical size={13} className="font-grip" aria-hidden />
              <span className="font-order" aria-hidden>
                {index + 1}
              </span>
              <span className="font-name" style={ownFace(family)}>
                {family}
              </span>
              <Tags font={font} locale={locale} />
              {missing && <span className="font-tag warn">{t.fontPicker.notInstalled}</span>}
              <span className="font-sample" style={sampleFace(family)} aria-hidden>
                {sampleText(font, locale)}
              </span>
              <button
                type="button"
                className="icon-button"
                aria-label={t.fontPicker.remove(family)}
                title={t.fontPicker.remove(family)}
                onClick={() => remove(index)}
              >
                <X size={13} />
              </button>
            </li>
          );
        })}
      </ol>
      {searching ? (
        <FontSearch
          fonts={fonts}
          status={installed.status}
          families={families}
          onAdd={(family) => change(addFamily(families, family))}
          onClose={() => {
            setSearching(false);
            setFocusAfter("");
          }}
        />
      ) : (
        <button ref={addButton} type="button" className="font-add" disabled={full} onClick={() => setSearching(true)}>
          <Plus size={13} />
          {full ? t.fontPicker.full(MAX_FONT_FAMILIES) : t.fontPicker.add}
        </button>
      )}
    </div>
  );
}

interface FontSearchProps {
  fonts: SystemFont[];
  status: "loading" | "ready" | "failed";
  families: string[];
  onAdd: (family: string) => void;
  onClose: () => void;
}

/** One row of the search: an installed font, or the typed name as it is. */
interface FontOption {
  family: string;
  font?: SystemFont;
}

/**
 * The search below the list. It stays open to add several fonts; Escape or
 * × closes it. Escape here does not close Settings (`data-escape-own`).
 */
function FontSearch({ fonts, status, families, onAdd, onClose }: FontSearchProps) {
  const { t, locale } = useI18n();
  const [query, setQuery] = useState("");
  const [monoOnly, setMonoOnly] = useState(true);
  const [active, setActive] = useState(0);
  const list = useRef<HTMLUListElement>(null);

  const chosen = new Set(families.map((family) => family.toLowerCase()));
  const typed = query.trim();
  const options: FontOption[] = searchFonts(fonts, query, monoOnly).map((font) => ({ family: font.family, font }));
  const exact = fonts.find((font) => font.family.toLowerCase() === typed.toLowerCase());
  if (exact && !options.some((option) => option.font === exact)) options.unshift({ family: exact.family, font: exact });
  if (typed && !exact && !typed.includes(",")) options.push({ family: typed });
  const at = Math.min(active, options.length - 1);
  const full = families.length >= MAX_FONT_FAMILIES;

  useEffect(() => {
    list.current?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  }, [at]);

  const pick = (option: FontOption | undefined) => {
    if (!option || chosen.has(option.family.toLowerCase())) return;
    onAdd(option.family);
    setQuery("");
    setActive(0);
  };

  const onKey = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing) return;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      setActive(Math.min(Math.max(at + step, 0), Math.max(options.length - 1, 0)));
    } else if (event.key === "Enter") {
      event.preventDefault();
      pick(options[at]);
    } else if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    }
  };

  let note: string | null = null;
  if (full) note = t.fontPicker.full(MAX_FONT_FAMILIES);
  else if (status === "loading") note = t.fontPicker.loading;
  else if (status === "failed") note = t.fontPicker.failed;
  else if (options.length === 0) note = t.fontPicker.noMatch;

  return (
    <div className="font-search" data-escape-own>
      <div className="font-search-bar">
        <Search size={13} aria-hidden />
        <input
          type="text"
          role="combobox"
          aria-label={t.fontPicker.search}
          aria-expanded="true"
          aria-controls="font-options"
          aria-activedescendant={at >= 0 ? `font-option-${at}` : undefined}
          placeholder={t.fontPicker.search}
          value={query}
          autoFocus
          spellCheck={false}
          autoComplete="off"
          onChange={(event) => {
            setQuery(event.target.value);
            setActive(0);
          }}
          onKeyDown={onKey}
        />
        <label className="settings-check">
          <input
            type="checkbox"
            checked={monoOnly}
            onChange={(event) => {
              setMonoOnly(event.target.checked);
              setActive(0);
            }}
          />
          {t.fontPicker.monoOnly}
        </label>
        <button type="button" className="icon-button" aria-label={t.fontPicker.close} title={t.fontPicker.close} onClick={onClose}>
          <X size={13} />
        </button>
      </div>
      {note && <p className="font-search-note">{note}</p>}
      {!full && options.length > 0 && (
        <ul id="font-options" ref={list} className="font-options" role="listbox" aria-label={t.fontPicker.search}>
          {options.map((option, index) => {
            const added = chosen.has(option.family.toLowerCase());
            return (
              <li
                key={`${option.font ? "font" : "typed"}:${option.family}`}
                id={`font-option-${index}`}
                role="option"
                className="font-option"
                aria-selected={index === at}
                aria-disabled={added || undefined}
                // Keep focus in the search field.
                onMouseDown={(event) => event.preventDefault()}
                onMouseMove={() => index !== at && setActive(index)}
                onClick={() => pick(option)}
              >
                {option.font ? (
                  <span className="font-name" style={ownFace(option.family)}>
                    {option.family}
                  </span>
                ) : (
                  <span className="font-name">{t.fontPicker.addTyped(option.family)}</span>
                )}
                <Tags font={option.font} locale={locale} />
                <span className="font-sample" style={sampleFace(option.family)} aria-hidden>
                  {sampleText(option.font, locale)}
                </span>
                {added && <Check size={13} className="font-added" aria-hidden />}
              </li>
            );
          })}
        </ul>
      )}
      {status === "ready" && (
        <p className="font-search-count">{t.fontPicker.count(options.filter((option) => option.font).length, fonts.length)}</p>
      )}
    </div>
  );
}
