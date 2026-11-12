import { BookOpen, CodeXml, Globe, Puzzle, Server, ServerCog, SquareTerminal, type LucideIcon } from "lucide-react";
import type { PaneKind } from "../shared/protocol.ts";

/** Labels and hints are UI text: see `kinds` in src/i18n/messages.ts. */
interface KindMeta {
  icon: LucideIcon;
  /** Decoded Kitty image budget per screen. Pixel apps keep full frames alive. */
  imageLimit: number;
}

const MiB = 1024 * 1024;

export const KINDS: Record<PaneKind, KindMeta> = {
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
  /** No terminal: the pane shows Markdown files. */
  docs: {
    icon: BookOpen,
    imageLimit: 0,
  },
  /** No terminal: a plugin draws the pane (src/plugins/PluginPaneHost.tsx). */
  plugin: {
    icon: Puzzle,
    imageLimit: 0,
  },
};

/** Launcher order in the activity bar. */
export const LAUNCHERS: PaneKind[] = ["shell", "browser", "editor", "ssh", "docs"];

export function KindIcon({ kind, size }: { kind: PaneKind; size: number }) {
  const Icon = KINDS[kind].icon;
  return <Icon size={size} strokeWidth={1.75} className={`kind-icon kind-${kind}`} aria-hidden="true" />;
}

/** The server icon of a remote window, for the buttons that open one. */
export function RemoteWindowIcon({ size }: { size: number }) {
  return <ServerCog size={size} strokeWidth={1.75} className="kind-icon kind-remote" aria-hidden="true" />;
}
