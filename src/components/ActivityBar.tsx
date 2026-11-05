import type { ReactNode } from "react";
import { Command, PanelLeftClose, PanelLeftOpen, Settings } from "lucide-react";
import type { SessionKind } from "../../shared/protocol.ts";
import { useI18n } from "../i18n/index.tsx";
import { KINDS, LAUNCHERS, RemoteWindowIcon } from "../kinds.tsx";

interface ActivityBarProps {
  sidebarOpen: boolean;
  counts: Record<SessionKind, number>;
  onLaunch: (kind: SessionKind) => void;
  onToggleSidebar: () => void;
  onOpenSettings: () => void;
  onOpenPalette: () => void;
  /** Picks a server to open in a new window. Null in a remote window: windows do not nest. */
  onOpenRemoteWindow: (() => void) | null;
  /** Shown in the palette button's tooltip, e.g. ⌘K. */
  paletteShortcut: string;
}

/**
 * The icon bar on the far left: launchers at the top; the command palette,
 * settings (language, theme, font, updates), and the sidebar switch at the
 * bottom.
 */
export function ActivityBar(props: ActivityBarProps) {
  const { sidebarOpen, counts, onLaunch, onToggleSidebar, onOpenSettings } = props;
  const { t } = useI18n();
  return (
    <nav className="activity-bar" aria-label={t.activity.launchers}>
      <div className="activity-group">
        {LAUNCHERS.map((kind) => {
          const { label, hint } = t.kinds[kind];
          const Icon = KINDS[kind].icon;
          return (
            <button
              key={kind}
              type="button"
              className="activity-button"
              data-kind={kind}
              aria-label={label}
              title={`${label} — ${hint}`}
              onClick={() => onLaunch(kind)}
            >
              <Icon size={20} strokeWidth={1.75} />
              {counts[kind] > 0 && (
                <span className="activity-badge" aria-hidden="true">
                  {counts[kind]}
                </span>
              )}
              <span className="activity-tooltip" role="presentation">
                {label}
              </span>
            </button>
          );
        })}
        {props.onOpenRemoteWindow && (
          <BarButton
            label={t.activity.remoteWindow}
            tooltip={t.activity.remoteWindow}
            title={`${t.activity.remoteWindow} — ${t.activity.remoteWindowHint}`}
            onClick={props.onOpenRemoteWindow}
          >
            <RemoteWindowIcon size={20} />
          </BarButton>
        )}
      </div>
      <div className="activity-group">
        <BarButton label={t.palette.open} tooltip={`${t.palette.open} · ${props.paletteShortcut}`} onClick={props.onOpenPalette}>
          <Command size={17} strokeWidth={1.75} />
        </BarButton>
        <BarButton label={t.activity.settings} tooltip={t.activity.settings} onClick={onOpenSettings}>
          <Settings size={18} strokeWidth={1.75} />
        </BarButton>
        <BarButton
          label={sidebarOpen ? t.activity.hideSessions : t.activity.showSessions}
          tooltip={sidebarOpen ? t.activity.hideSessions : t.activity.showSessions}
          pressed={sidebarOpen}
          onClick={onToggleSidebar}
        >
          {sidebarOpen ? <PanelLeftClose size={18} strokeWidth={1.75} /> : <PanelLeftOpen size={18} strokeWidth={1.75} />}
        </BarButton>
      </div>
    </nav>
  );
}

interface BarButtonProps {
  label: string;
  tooltip: string;
  /** Native tooltip, for a longer text than the styled one. */
  title?: string;
  pressed?: boolean;
  onClick: () => void;
  children: ReactNode;
}

function BarButton({ label, tooltip, title, pressed, onClick, children }: BarButtonProps) {
  return (
    <button
      type="button"
      className="activity-button"
      aria-label={label}
      title={title}
      aria-pressed={pressed}
      onClick={onClick}
    >
      {children}
      <span className="activity-tooltip" role="presentation">
        {tooltip}
      </span>
    </button>
  );
}
