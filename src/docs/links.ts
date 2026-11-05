// Where a link in a rendered document leads. Only Markdown files inside the
// root open in the docs pane; web pages open in a terminal-browser pane.

export type LinkTarget =
  | { type: "doc"; path: string; hash: string | null }
  | { type: "anchor"; id: string }
  | { type: "external"; url: string };

function decode(text: string): string {
  try {
    return decodeURIComponent(text);
  } catch {
    return text;
  }
}

/**
 * `href` from a document at `currentFile` (relative to the root, `/`-separated).
 * Relative paths start from the file's folder, `/…` from the root. Null for
 * a link that leaves the root, names no Markdown file, or uses another scheme.
 */
export function resolveLink(currentFile: string, href: string): LinkTarget | null {
  const link = href.trim();
  if (link.startsWith("#")) {
    const id = decode(link.slice(1));
    return id ? { type: "anchor", id } : null;
  }
  if (/^[a-z][a-z0-9+.-]*:/i.test(link)) {
    return /^https?:\/\//i.test(link) ? { type: "external", url: link } : null;
  }
  if (link.startsWith("//")) return null;
  const hashAt = link.indexOf("#");
  const beforeHash = hashAt < 0 ? link : link.slice(0, hashAt);
  const hash = hashAt < 0 ? null : decode(link.slice(hashAt + 1)) || null;
  const pathPart = decode(beforeHash.split("?")[0]);
  if (!/\.(md|markdown)$/i.test(pathPart)) return null;

  const parts = pathPart.startsWith("/") ? [] : currentFile.split("/").slice(0, -1);
  for (const part of pathPart.split("/")) {
    if (part === "" || part === ".") continue;
    if (part === "..") {
      if (parts.length === 0) return null;
      parts.pop();
    } else {
      parts.push(part);
    }
  }
  return parts.length > 0 ? { type: "doc", path: parts.join("/"), hash } : null;
}
