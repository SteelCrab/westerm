import type { SessionStatus } from "../workspace.ts";

const LABEL: Record<SessionStatus, string> = {
  connecting: "연결 중",
  running: "실행 중",
  disconnected: "다시 연결 중",
  exited: "종료됨",
  error: "오류",
};

export function StatusDot({ status }: { status: SessionStatus }) {
  return (
    <span className={`status-dot ${status}`} title={LABEL[status]} aria-label={LABEL[status]} role="img" />
  );
}
