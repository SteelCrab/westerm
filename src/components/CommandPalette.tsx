import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Search } from "lucide-react";

export interface PaletteCommand {
  id: string;
  title: string;
  /** Shown on the right: a shortcut or where the target lives. */
  hint?: string;
  group: string;
  icon?: ReactNode;
  /** Extra words to match, e.g. English names for Korean titles. */
  keywords?: string;
  run: () => void;
}

interface CommandPaletteProps {
  commands: PaletteCommand[];
  /** Commands that depend on the typed text, such as "open this URL". */
  dynamic: (query: string) => PaletteCommand[];
  placeholder?: string;
  /** Shown when nothing matches. */
  empty?: string;
  onClose: () => void;
}

/**
 * Fuzzy subsequence score, or -1 when `query` is not a subsequence of `text`.
 * Consecutive characters and word starts score higher.
 */
export function fuzzyScore(text: string, query: string): number {
  const haystack = text.toLowerCase();
  const needle = query.toLowerCase().replace(/\s+/g, "");
  if (!needle) return 0;
  let score = 0;
  let from = 0;
  let previous = -2;
  for (const char of needle) {
    const at = haystack.indexOf(char, from);
    if (at < 0) return -1;
    score += 1;
    if (at === previous + 1) score += 3;
    if (at === 0 || /[\s:/._-]/.test(haystack[at - 1])) score += 2;
    previous = at;
    from = at + 1;
  }
  // Prefer shorter, tighter matches.
  return score - haystack.length * 0.01;
}

const MAX_RESULTS = 50;

export function CommandPalette({ commands, dynamic, placeholder, empty, onClose }: CommandPaletteProps) {
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const listRef = useRef<HTMLUListElement>(null);

  const results = useMemo(() => {
    const extra = dynamic(query);
    if (!query.trim()) return [...extra, ...commands].slice(0, MAX_RESULTS);
    const scored = commands
      .map((command) => {
        const score = Math.max(
          fuzzyScore(command.title, query),
          fuzzyScore(`${command.group} ${command.keywords ?? ""}`, query) - 1,
        );
        return { command, score };
      })
      .filter((entry) => entry.score >= 0)
      .sort((a, b) => b.score - a.score)
      .map((entry) => entry.command);
    return [...extra, ...scored].slice(0, MAX_RESULTS);
  }, [commands, dynamic, query]);

  useEffect(() => setSelected(0), [query]);

  useEffect(() => {
    listRef.current?.querySelector<HTMLElement>(`[data-index="${selected}"]`)?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  const run = (command: PaletteCommand | undefined) => {
    if (!command) return;
    onClose();
    command.run();
  };

  return (
    <div className="palette-backdrop" onMouseDown={onClose}>
      <div
        className="palette"
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="palette-input-row">
          <Search size={15} aria-hidden="true" />
          <input
            autoFocus
            className="palette-input"
            placeholder={placeholder ?? "명령 검색, 또는 URL을 입력해 terminal-browser로 열기"}
            value={query}
            role="combobox"
            aria-expanded="true"
            aria-controls="palette-results"
            aria-activedescendant={results[selected] ? `palette-item-${selected}` : undefined}
            spellCheck={false}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              event.stopPropagation();
              if (event.nativeEvent.isComposing) return;
              if (event.key === "ArrowDown") {
                event.preventDefault();
                setSelected((index) => Math.min(results.length - 1, index + 1));
              } else if (event.key === "ArrowUp") {
                event.preventDefault();
                setSelected((index) => Math.max(0, index - 1));
              } else if (event.key === "Enter") {
                event.preventDefault();
                run(results[selected]);
              } else if (event.key === "Escape") {
                event.preventDefault();
                onClose();
              }
            }}
          />
        </div>
        <ul className="palette-results" id="palette-results" role="listbox" ref={listRef}>
          {results.length === 0 && <li className="palette-empty">{empty ?? "일치하는 명령이 없습니다"}</li>}
          {results.map((command, index) => (
            <li
              key={command.id}
              id={`palette-item-${index}`}
              data-index={index}
              role="option"
              aria-selected={index === selected}
              className={`palette-item${index === selected ? " selected" : ""}`}
              onMouseMove={() => index !== selected && setSelected(index)}
              onClick={() => run(command)}
            >
              <span className="palette-icon" aria-hidden="true">
                {command.icon}
              </span>
              <span className="palette-title">{command.title}</span>
              <span className="palette-group">{command.group}</span>
              {command.hint && <kbd className="palette-hint">{command.hint}</kbd>}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
