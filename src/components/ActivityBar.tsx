import { PanelLeftClose, PanelLeftOpen } from "lucide-react";
import type { SessionKind } from "../../shared/protocol.ts";
import { KINDS, LAUNCHERS } from "../kinds.tsx";

interface ActivityBarProps {
  sidebarOpen: boolean;
  counts: Record<SessionKind, number>;
  onLaunch: (kind: SessionKind) => void;
  onToggleSidebar: () => void;
}

/** Narrow icon rail on the far left. Each launcher starts a new pane. */
export function ActivityBar({ sidebarOpen, counts, onLaunch, onToggleSidebar }: ActivityBarProps) {
  return (
    <nav className="activity-bar" aria-label="Launchers">
      <div className="activity-group">
        {LAUNCHERS.map((kind) => {
          const { label, hint, icon: Icon } = KINDS[kind];
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
          aria-label={sidebarOpen ? "Hide sessions" : "Show sessions"}
          aria-pressed={sidebarOpen}
          title={sidebarOpen ? "세션 목록 숨기기" : "세션 목록 보기"}
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
