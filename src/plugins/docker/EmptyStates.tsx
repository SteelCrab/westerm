// What the Docker panel shows in place of the list: no CLI, engine down, no containers.

import type { ReactNode } from "react";
import { Container } from "lucide-react";
import type { CopyResult } from "../../useCopy.ts";
import type { DockerMessages } from "./i18n.ts";
import { dockerInstallCommand } from "./model.ts";

export type Platform = "mac" | "windows" | "linux";

const INSTALL_DOCS = "https://docs.docker.com/engine/install/";

/** The platform the browser runs on, for the install command. */
export function detectPlatform(): Platform {
  if (typeof navigator === "undefined") return "linux";
  const name = `${navigator.platform} ${navigator.userAgent}`;
  if (/Mac|iPhone|iPad/.test(name)) return "mac";
  return /Win/.test(name) ? "windows" : "linux";
}

/** Docker's errors can run to many lines; the panel shows the first. */
export const firstLine = (text: string) => text.split(/\r?\n/, 1)[0];

/** A title, one line of detail (the full text on hover), and a button or two. */
export function EmptyState({ title, detail, children }: { title: string; detail: string | null; children?: ReactNode }) {
  return (
    <div className="docker-empty">
      <Container size={28} className="docker-empty-icon" aria-hidden="true" />
      <p className="docker-empty-title">{title}</p>
      {detail && (
        <p className="docker-empty-detail" title={detail}>
          {firstLine(detail)}
        </p>
      )}
      {children}
    </div>
  );
}

interface InstallHintProps {
  t: DockerMessages;
  platform: Platform;
  copied: CopyResult;
  onCopy: (text: string) => void;
}

/** The install command with a Copy button; Linux gets the docs link instead. */
export function InstallHint({ t, platform, copied, onCopy }: InstallHintProps) {
  const command = dockerInstallCommand(platform);
  if (command === null) {
    return (
      <a className="docker-install-link" href={INSTALL_DOCS} target="_blank" rel="noreferrer">
        {t.installGuide}
      </a>
    );
  }
  return (
    <div className="docker-install">
      <code>{command}</code>
      <button type="button" className="button" onClick={() => onCopy(command)}>
        {copied === "copied" ? t.copied : copied === "failed" ? t.copyFailed : t.copy}
      </button>
    </div>
  );
}
