import { CodeXml, Globe, Server, SquareTerminal, type LucideIcon } from "lucide-react";
import type { SessionKind } from "../shared/protocol.ts";

interface KindMeta {
  /** Launcher label in the activity bar. */
  label: string;
  hint: string;
  icon: LucideIcon;
  /** Decoded Kitty image budget per screen. Pixel apps keep full frames alive. */
  imageLimit: number;
}

const MiB = 1024 * 1024;

export const KINDS: Record<SessionKind, KindMeta> = {
  shell: {
    label: "New shell",
    hint: "새 셸 세션을 엽니다",
    icon: SquareTerminal,
    imageLimit: 32 * MiB,
  },
  browser: {
    label: "terminal-browser",
    hint: "터미널 안에서 브라우저를 실행합니다",
    icon: Globe,
    imageLimit: 96 * MiB,
  },
  editor: {
    label: "terminal-code",
    hint: "포커스된 셸의 폴더를 VS Code로 엽니다",
    icon: CodeXml,
    imageLimit: 96 * MiB,
  },
  ssh: {
    label: "SSH",
    hint: "~/.ssh/config의 호스트에 접속합니다",
    icon: Server,
    imageLimit: 32 * MiB,
  },
};

/** Launcher order in the activity bar. */
export const LAUNCHERS: SessionKind[] = ["shell", "browser", "editor", "ssh"];

export function KindIcon({ kind, size }: { kind: SessionKind; size: number }) {
  const Icon = KINDS[kind].icon;
  return <Icon size={size} strokeWidth={1.75} className={`kind-icon kind-${kind}`} aria-hidden="true" />;
}
