import { Fragment, useEffect, useRef, useState } from "react";
import {
  BookOpen,
  Bug,
  Check,
  ChevronDown,
  Code,
  Database,
  FlaskConical,
  Folder,
  Globe,
  Layers,
  Pencil,
  Plus,
  Rocket,
  Server,
  Star,
  Terminal,
  X,
  type LucideIcon,
  type LucideProps,
} from "lucide-react";
import { WORKSPACE_ICONS, type WorkspaceIcon } from "../../shared/workspace-file.ts";
import { useI18n } from "../i18n/index.tsx";
import { activeWorkspace, workspaceLabel, type Dashboard } from "../workspace.ts";
import { InlineRename, type RenameTarget } from "./InlineRename.tsx";

interface WorkspaceMenuProps {
  state: Dashboard;
  renaming: RenameTarget | null;
  onRename: (target: RenameTarget | null) => void;
  onSelect: (id: string) => void;
  onNew: () => void;
  onClose: (id: string) => void;
  onRenameWorkspace: (id: string, name: string) => void;
  onSetIcon: (id: string, icon: WorkspaceIcon) => void;
  /** Where the menu is: at the left of the tab bar, or at the top of the sidebar. */
  where: "tabbar" | "sidebar";
  /** The button shows the workspace's name too (the sidebar, unless dragged narrow). */
  labelled: boolean;
}

const ICONS: Record<WorkspaceIcon, LucideIcon> = {
  layers: Layers,
  folder: Folder,
  code: Code,
  terminal: Terminal,
  globe: Globe,
  server: Server,
  database: Database,
  book: BookOpen,
  flask: FlaskConical,
  rocket: Rocket,
  bug: Bug,
  star: Star,
};

/** A workspace's icon, decorative: the name beside it says which workspace. */
export function WorkspaceGlyph({ icon, ...rest }: { icon: WorkspaceIcon } & LucideProps) {
  const Icon = ICONS[icon];
  return <Icon aria-hidden="true" {...rest} />;
}

// Theme colors that already have light and dark values.
const COLORS = ["--accent", "--browser", "--editor", "--ssh", "--warn", "--danger"];

/** The icon color of the workspace at this position. */
export function workspaceColor(index: number): string {
  return `var(${COLORS[index % COLORS.length]})`;
}

/** The menu item that an arrow key moves to; -1 is "none focused yet". */
export function stepIndex(current: number, delta: 1 | -1, count: number): number {
  if (current < 0) return delta > 0 ? 0 : count - 1;
  return (current + delta + count) % count;
}

/**
 * The workspaces as a menu: at the left of the tab bar in the top layout, at
 * the top of the sidebar in the sidebar layout. A click shows a workspace and
 * closes the menu, so renaming is the pencil button or F2, not a double-click.
 */
export function WorkspaceMenu(props: WorkspaceMenuProps) {
  const { state, renaming } = props;
  const { t } = useI18n();
  const root = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  // The workspace whose icon choices show below its row.
  const [picking, setPicking] = useState<string | null>(null);
  const editingId = renaming?.kind === "workspace" && renaming.where === props.where ? renaming.id : null;
  // The palette's "Rename workspace" opens the menu with that row as a field.
  const shown = open || editingId !== null;
  const current = activeWorkspace(state);
  const currentIndex = state.workspaces.indexOf(current);
  const currentLabel = workspaceLabel(state, current, t.workspaces.untitled);

  const close = (refocus: boolean) => {
    setOpen(false);
    setPicking(null);
    if (refocus) button.current?.focus();
  };

  useEffect(() => {
    if (!open) return;
    root.current?.querySelector<HTMLElement>('[aria-checked="true"]')?.focus();
    const onPointer = (event: PointerEvent) => {
      if (root.current?.contains(event.target as Node)) return;
      setOpen(false);
      setPicking(null);
    };
    window.addEventListener("pointerdown", onPointer, true);
    return () => window.removeEventListener("pointerdown", onPointer, true);
  }, [open]);

  const onKeyDown = (event: React.KeyboardEvent) => {
    // Keep keys away from the terminal and the tab shortcuts.
    event.stopPropagation();
    if (event.key === "Escape") {
      event.preventDefault();
      // The icon choices close first, then the menu.
      if (picking) setPicking(null);
      else close(true);
      return;
    }
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    const items = Array.from(root.current?.querySelectorAll<HTMLElement>("[data-menu-item]") ?? []);
    const next = stepIndex(items.indexOf(document.activeElement as HTMLElement), event.key === "ArrowDown" ? 1 : -1, items.length);
    items[next]?.focus();
  };

  return (
    <div className={`ws-menu in-${props.where}`} ref={root}>
      <button
        ref={button}
        type="button"
        className={`ws-button${props.labelled ? " labelled" : ""}`}
        aria-haspopup="menu"
        aria-expanded={shown}
        aria-label={t.workspaces.switcher(currentLabel)}
        title={t.workspaces.switcher(currentLabel)}
        style={{ "--ws-color": workspaceColor(currentIndex) } as React.CSSProperties}
        onClick={() => (open ? close(false) : setOpen(true))}
      >
        <WorkspaceGlyph icon={current.icon} size={15} className="ws-icon" />
        {props.labelled && (
          <>
            <span className="ws-name">{currentLabel}</span>
            <ChevronDown size={14} className="ws-chevron" aria-hidden="true" />
          </>
        )}
      </button>
      {shown && (
        <div className="ws-panel" role="menu" aria-label={t.workspaces.switchTitle} onKeyDown={onKeyDown}>
          <div className="ws-panel-title" aria-hidden="true">
            {t.workspaces.switchTitle}
          </div>
          {state.workspaces.map((workspace, index) => {
            const active = workspace.id === state.activeWorkspaceId;
            const label = workspaceLabel(state, workspace, t.workspaces.untitled);
            const startRename = () => props.onRename({ kind: "workspace", id: workspace.id, where: props.where });
            const color = { "--ws-color": workspaceColor(index) } as React.CSSProperties;
            return (
              <Fragment key={workspace.id}>
                <div role="none" className={`ws-item${active ? " active" : ""}`} style={color}>
                  {editingId === workspace.id ? (
                    <>
                      <WorkspaceGlyph icon={workspace.icon} size={14} className="ws-icon" />
                      <InlineRename
                        value={workspace.name ?? ""}
                        label={t.rename.workspaceName}
                        className="ws-rename"
                        // Ending a rename gives the keyboard back to the terminal,
                        // so the menu closes too.
                        onCommit={(name) => {
                          props.onRenameWorkspace(workspace.id, name);
                          props.onRename(null);
                          setOpen(false);
                        }}
                        onCancel={() => {
                          props.onRename(null);
                          setOpen(false);
                        }}
                      />
                    </>
                  ) : (
                    <>
                      <button
                        type="button"
                        className="ws-icon-button"
                        tabIndex={-1}
                        aria-label={t.workspaces.changeIcon(label)}
                        aria-expanded={picking === workspace.id}
                        title={t.workspaces.changeIcon(label)}
                        onClick={() => setPicking(picking === workspace.id ? null : workspace.id)}
                      >
                        <WorkspaceGlyph icon={workspace.icon} size={14} className="ws-icon" />
                      </button>
                      <button
                        type="button"
                        role="menuitemradio"
                        aria-checked={active}
                        className="ws-select"
                        data-menu-item
                        title={label}
                        onClick={() => {
                          props.onSelect(workspace.id);
                          close(false);
                        }}
                        onKeyDown={(event) => event.key === "F2" && startRename()}
                      >
                        <span className={`ws-name${workspace.name ? " custom" : ""}`}>{label}</span>
                        {active && <Check size={14} className="ws-check" aria-hidden="true" />}
                      </button>
                    </>
                  )}
                  <button
                    type="button"
                    className="icon-button subtle ws-action"
                    tabIndex={-1}
                    aria-label={t.workspaces.rename(label)}
                    title={t.workspaces.rename(label)}
                    onClick={startRename}
                  >
                    <Pencil size={12} />
                  </button>
                  <button
                    type="button"
                    className="icon-button subtle ws-action"
                    tabIndex={-1}
                    aria-label={t.workspaces.close(label)}
                    title={t.workspaces.closeHint}
                    onClick={() => props.onClose(workspace.id)}
                  >
                    <X size={13} />
                  </button>
                </div>
                {picking === workspace.id && (
                  <div className="ws-icons" role="group" aria-label={t.workspaces.changeIcon(label)} style={color}>
                    {WORKSPACE_ICONS.map((icon) => (
                      <button
                        key={icon}
                        type="button"
                        className="ws-icon-option"
                        aria-label={t.workspaces.icons[icon]}
                        aria-pressed={workspace.icon === icon}
                        title={t.workspaces.icons[icon]}
                        onClick={() => {
                          props.onSetIcon(workspace.id, icon);
                          setPicking(null);
                        }}
                      >
                        <WorkspaceGlyph icon={icon} size={14} />
                      </button>
                    ))}
                  </div>
                )}
              </Fragment>
            );
          })}
          <div className="ws-separator" role="separator" />
          <button
            type="button"
            role="menuitem"
            className="ws-select ws-create"
            data-menu-item
            onClick={() => {
              props.onNew();
              close(false);
            }}
          >
            <Plus size={14} aria-hidden="true" />
            <span className="ws-name">{t.workspaces.newWorkspace}</span>
          </button>
        </div>
      )}
    </div>
  );
}
