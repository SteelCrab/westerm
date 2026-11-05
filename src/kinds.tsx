import { CodeXml, Globe, Server, ServerCog, SquareTerminal, type LucideIcon } from "lucide-react";
import type { SessionKind } from "../shared/protocol.ts";

/** Labels and hints are UI text: see `kinds` in src/i18n/messages.ts. */
interface KindMeta {
  icon: LucideIcon;
  /** Decoded Kitty image budget per screen. Pixel apps keep full frames alive. */
  imageLimit: number;
}

const MiB = 1024 * 1024;

export const KINDS: Record<SessionKind, KindMeta> = {
  shell: {
    icon: SquareTerminal,
    imageLimit: 32 * MiB,
  },
  browser: {
    icon: Globe,
    imageLimit: 96 * MiB,
  },
  editor: {
    icon: CodeXml,
    imageLimit: 96 * MiB,
  },
  ssh: {
    icon: Server,
    imageLimit: 32 * MiB,
  },
  remote: {
    icon: ServerCog,
    imageLimit: 32 * MiB,
  },
};

/** Launcher order in the activity bar. */
export const LAUNCHERS: SessionKind[] = ["shell", "browser", "editor", "ssh", "remote"];

export function KindIcon({ kind, size }: { kind: SessionKind; size: number }) {
  const Icon = KINDS[kind].icon;
  return <Icon size={size} strokeWidth={1.75} className={`kind-icon kind-${kind}`} aria-hidden="true" />;
}
