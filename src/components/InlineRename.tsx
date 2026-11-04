import { useEffect, useRef } from "react";

/** Where a rename was started; the input appears in that spot. */
export type RenameTarget =
  | { kind: "tab"; id: string; where: "tabbar" | "sidebar" }
  | { kind: "session"; id: string; where: "pane" | "sidebar" };

interface InlineRenameProps {
  /** Current label, preselected. */
  value: string;
  label: string;
  className?: string;
  /** Empty text clears the custom name. */
  onCommit: (name: string) => void;
  onCancel: () => void;
}

/** Text field that replaces a label: Enter or blur saves, Escape cancels. */
export function InlineRename({ value, label, className, onCommit, onCancel }: InlineRenameProps) {
  const ref = useRef<HTMLInputElement>(null);
  const done = useRef(false);

  useEffect(() => {
    ref.current?.focus();
    ref.current?.select();
  }, []);

  const finish = (commit: boolean) => {
    if (done.current) return;
    done.current = true;
    if (commit) onCommit(ref.current?.value ?? "");
    else onCancel();
  };

  return (
    <input
      ref={ref}
      className={`inline-rename${className ? ` ${className}` : ""}`}
      defaultValue={value}
      aria-label={label}
      placeholder="비우면 자동 이름"
      maxLength={120}
      spellCheck={false}
      onKeyDown={(event) => {
        // Keep keys away from the terminal and the tab/pane shortcuts.
        event.stopPropagation();
        if (event.key === "Enter" && !event.nativeEvent.isComposing) finish(true);
        else if (event.key === "Escape") finish(false);
      }}
      onBlur={() => finish(true)}
      onClick={(event) => event.stopPropagation()}
      onDoubleClick={(event) => event.stopPropagation()}
      onMouseDown={(event) => event.stopPropagation()}
    />
  );
}
