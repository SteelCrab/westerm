// Markdown to HTML for the docs pane. Raw HTML in a document shows as text,
// links with unsafe schemes are dropped (markdown-it's validateLink), and
// images show their alt text: the pane does not load files besides `.md`.

import MarkdownIt from "markdown-it";

/** A heading's anchor id, the way GitHub makes them: any script's letters and digits, `-`, `_`. */
export function slug(text: string): string {
  return text
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\p{M}\s_-]/gu, "")
    .replace(/\s/g, "-");
}

interface RenderEnv {
  /** How often each id was used in this document, for `-1`, `-2` on repeats. */
  ids: Map<string, number>;
}

const md = new MarkdownIt({ html: false, linkify: true });

md.renderer.rules.heading_open = (tokens, idx, options, env: RenderEnv, self) => {
  const inline = tokens[idx + 1];
  const text = (inline?.children ?? [])
    .filter((child) => child.type === "text" || child.type === "code_inline")
    .map((child) => child.content)
    .join("");
  const base = slug(text);
  if (base) {
    const seen = env.ids.get(base) ?? 0;
    env.ids.set(base, seen + 1);
    tokens[idx].attrSet("id", seen === 0 ? base : `${base}-${seen}`);
  }
  return self.renderToken(tokens, idx, options);
};

md.renderer.rules.image = (tokens, idx, options, env, self) => {
  const alt = self.renderInlineAsText(tokens[idx].children ?? [], options, env);
  return `<span class="md-image">${md.utils.escapeHtml(alt)}</span>`;
};

export function renderMarkdown(source: string): string {
  const env: RenderEnv = { ids: new Map() };
  return md.render(source, env);
}
