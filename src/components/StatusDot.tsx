import { useI18n } from "../i18n/index.tsx";
import type { SessionStatus } from "../workspace.ts";

export function StatusDot({ status }: { status: SessionStatus }) {
  const label = useI18n().t.status[status];
  return <span className={`status-dot ${status}`} title={label} aria-label={label} role="img" />;
}
