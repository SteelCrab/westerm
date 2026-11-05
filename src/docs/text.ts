// A document's text between the disk and the editor, and the file tree.

/**
 * A textarea turns `\r\n` into `\n`. The editor works on `\n` text and
 * remembers whether the file used `\r\n`, so a save writes them back.
 */
export function fromDisk(content: string): { text: string; crlf: boolean } {
  return { text: content.replace(/\r\n/g, "\n"), crlf: content.includes("\r\n") };
}

export function toDisk(text: string, crlf: boolean): string {
  const plain = text.replace(/\r\n/g, "\n");
  return crlf ? plain.replace(/\n/g, "\r\n") : plain;
}

export interface TreeNode {
  /** The folder's own name; "" for the root. */
  name: string;
  /** Relative to the root with `/`; "" for the root. */
  path: string;
  /** Sub-folders by name. */
  dirs: TreeNode[];
  /** Full relative paths of the files right in this folder, by name. */
  files: string[];
  /** Files in this folder and below. */
  count: number;
}

const byName = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);

/** The folders of `files` (relative paths), keeping those whose path contains `filter`, ignoring case. */
export function treeOf(files: readonly string[], filter: string): TreeNode {
  const root: TreeNode = { name: "", path: "", dirs: [], files: [], count: 0 };
  const needle = filter.trim().toLowerCase();
  for (const file of files) {
    if (needle && !file.toLowerCase().includes(needle)) continue;
    const parts = file.split("/");
    let node = root;
    node.count += 1;
    for (const part of parts.slice(0, -1)) {
      const path = node.path ? `${node.path}/${part}` : part;
      let next = node.dirs.find((dir) => dir.name === part);
      if (!next) {
        next = { name: part, path, dirs: [], files: [], count: 0 };
        node.dirs.push(next);
      }
      node = next;
      node.count += 1;
    }
    node.files.push(file);
  }
  const sort = (node: TreeNode) => {
    node.dirs.sort((a, b) => byName(a.name, b.name));
    node.files.sort((a, b) => byName(a.split("/").pop() ?? a, b.split("/").pop() ?? b));
    node.dirs.forEach(sort);
  };
  sort(root);
  return root;
}
