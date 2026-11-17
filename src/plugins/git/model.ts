// What the Git plugin's server routes answer, and how the panel groups it.

export interface GitFile {
  path: string;
  oldPath: string | null;
  index: string;
  worktree: string;
  conflict: boolean;
}

export interface GitStatus {
  root: string;
  branch: string | null;
  head: string | null;
  branches: string[];
  upstream: string | null;
  ahead: number;
  behind: number;
  files: GitFile[];
  commits: { hash: string; message: string; author: string; time: string }[];
}

/** A repository below a folder. A failed status leaves `changes` null and says why in `error`. */
export interface GitFoundRepository {
  path: string;
  /** The path from the folder, with `/`. */
  name: string;
  branch: string | null;
  head: string | null;
  changes: number | null;
  upstream: string | null;
  ahead: number;
  behind: number;
  error: string | null;
}

/** The repositories up to two folders below `root`. */
export interface GitFolder {
  root: string;
  /** The worktree that holds `root`; null outside one. */
  repository: string | null;
  repos: GitFoundRepository[];
  truncated: boolean;
}

/** The last part of a path. */
export const lastName = (path: string) => path.split(/[\\/]/).filter(Boolean).pop() ?? path;

export interface GitEntry extends GitFile {
  staged: boolean;
  status: string;
}

export interface GitCommitDetail {
  hash: string;
  message: string;
  author: string;
  email: string;
  time: string;
  parents: string[];
  files: { path: string; oldPath: string | null; status: string }[];
}

export type GitOperation =
  | { action: "stage" | "unstage"; paths: string[] }
  | { action: "commit"; message: string }
  | { action: "switch"; branch: string }
  | { action: "pull" | "push" };

/** A partially staged file appears in both groups, with independent diffs. */
export function gitEntries(files: GitFile[]): { staged: GitEntry[]; changes: GitEntry[] } {
  const staged: GitEntry[] = [];
  const changes: GitEntry[] = [];
  for (const file of files) {
    if (!file.conflict && file.index !== " " && file.index !== "?") staged.push({ ...file, staged: true, status: file.index });
    if (file.conflict || (file.worktree !== " " && file.worktree !== "")) {
      changes.push({ ...file, staged: false, status: file.conflict ? "U" : file.worktree });
    }
  }
  return { staged, changes };
}

export const sameGitEntry = (a: Pick<GitEntry, "path" | "staged"> | null, b: Pick<GitEntry, "path" | "staged">) =>
  a?.path === b.path && a.staged === b.staged;
