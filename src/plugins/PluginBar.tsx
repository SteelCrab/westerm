import { Puzzle, type LucideIcon } from "lucide-react";
import { useI18n } from "../i18n/index.tsx";
import type { PluginStatus } from "./types.ts";

export interface PluginButton {
  id: string;
  label: string;
  icon: LucideIcon;
  /** An image instead of `icon` (an external plugin's SVG). */
  iconUrl?: string;
  /** Installed from outside westerm: a small yellow mark. */
  external: boolean;
  /** Why the plugin is not running; shown with its name. */
  note?: string;
  status: PluginStatus;
  /** The sidebar slot shows this plugin's panel. */
  pressed: boolean;
}

interface PluginBarProps {
  plugins: PluginButton[];
  onTogglePlugin: (id: string) => void;
  /** Null where there is no plugin manager (a remote window, a tunnel page). */
  onOpenPlugins: (() => void) | null;
}

/**
 * The middle of the activity bar: a button per installed plugin, in registry
 * order, then the puzzle button that opens the plugin manager (where the
 * page has one).
 */
export function PluginBar({ plugins, onTogglePlugin, onOpenPlugins }: PluginBarProps) {
  const { t } = useI18n();
  return (
    <div className="plugin-bar">
      {plugins.map(({ id, label, icon: Icon, iconUrl, external, note, status, pressed }) => {
        const described = note ? `${label} · ${note}` : label;
        return (
          <button
            key={id}
            type="button"
            className="activity-button plugin-button"
            aria-label={status.badge === null ? described : `${described} · ${status.badge}`}
            aria-pressed={pressed}
            onClick={() => onTogglePlugin(id)}
          >
            {iconUrl ? (
              <img src={iconUrl} alt="" width="19" height="19" className="plugin-icon" />
            ) : (
              <Icon size={19} strokeWidth={1.75} />
            )}
            {external && <span className="plugin-external" aria-hidden="true" />}
            {status.badge !== null && (
              <span className="activity-badge" aria-hidden="true">
                {status.badge}
              </span>
            )}
            {status.alert && <span className="plugin-alert" aria-hidden="true" />}
            <span className="activity-tooltip" role="presentation">
              {described}
            </span>
          </button>
        );
      })}
      {onOpenPlugins && (
        <button type="button" className="activity-button plugin-open" aria-label={t.plugins.open} onClick={onOpenPlugins}>
          <Puzzle size={18} strokeWidth={1.75} />
          <span className="activity-tooltip" role="presentation">
            {t.plugins.open}
          </span>
        </button>
      )}
    </div>
  );
}
