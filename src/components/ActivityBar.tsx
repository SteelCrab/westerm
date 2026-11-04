import { PanelLeftClose, PanelLeftOpen } from "lucide-react";
import type { SessionKind } from "../../shared/protocol.ts";
import { useI18n } from "../i18n/index.tsx";
import { KINDS, LAUNCHERS } from "../kinds.tsx";

interface ActivityBarProps {
  sidebarOpen: boolean;
  counts: Record<SessionKind, number>;
  onLaunch: (kind: SessionKind) => void;
  onToggleSidebar: () => void;
}

/** Narrow icon rail on the far left. Each launcher starts a new pane. */
export function ActivityBar({ sidebarOpen, counts, onLaunch, onToggleSidebar }: ActivityBarProps) {
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
      </div>
      <div className="activity-group">
        <button
          type="button"
          className="activity-button"
          aria-label={sidebarOpen ? t.activity.hideSessions : t.activity.showSessions}
          aria-pressed={sidebarOpen}
          title={sidebarOpen ? t.activity.hideSessions : t.activity.showSessions}
          onClick={onToggleSidebar}
        >
          {sidebarOpen ? (
            <PanelLeftClose size={18} strokeWidth={1.75} />
          ) : (
            <PanelLeftOpen size={18} strokeWidth={1.75} />
          )}
        </button>
      </div>
    </nav>
  );
}
