import { useEffect, useRef, useState } from "react";
import {
  ArrowDown, ArrowUp, Check, ChevronDown, ChevronRight, FileCode2, FileText,
  FolderGit2, GitBranch, GitCommitHorizontal, Minus, Plus, RotateCcw,
} from "lucide-react";
import { useI18n } from "../i18n/index.tsx";
import {
  fetchGitCommit, fetchGitCommitDiff, fetchGitDiff, fetchGitStatus, focusedGitFolder, gitEntries, gitFolders, runGitOperation, sameGitEntry,
  type GitCommitDetail, type GitEntry, type GitOperation, type GitStatus,
} from "../git.ts";
import type { Dashboard } from "../workspace.ts";

export function GitPanel({ compact, state }: { compact: boolean; state: Dashboard }) {
  const { t, locale } = useI18n();
  const g = t.git;
  const folders = gitFolders(state);
  const [chosen, setChosen] = useState<string | null>(null);
  const dir = chosen ?? focusedGitFolder(state);
  const currentDir = useRef(dir);
  currentDir.current = dir;
  const [folderInput, setFolderInput] = useState("");
  const [loaded, setLoaded] = useState<{ dir: string; repo: GitStatus } | null>(null);
  const [error, setError] = useState("");
  const [loadError, setLoadError] = useState("");
  const [loading, setLoading] = useState(false);
  const [revision, setRevision] = useState(0);
  const [selected, setSelected] = useState<Pick<GitEntry, "path" | "staged"> | null>(null);
  const [selectedCommit, setSelectedCommit] = useState<string | null>(null);
  const [message, setMessage] = useState("");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const repo = loaded?.dir === dir ? loaded.repo : null;
  const { staged, changes } = gitEntries(repo?.files ?? []);
  const current = [...staged, ...changes].find((file) => sameGitEntry(selected, file)) ?? null;

  useEffect(() => {
    setSelected(null);
    setSelectedCommit(null);
    setMessage("");
    setNotice("");
    setError("");
    setFolderInput(dir ?? "");
  }, [dir]);

  useEffect(() => {
    setLoadError("");
    if (!dir) { setLoading(false); return; }
    let cancelled = false;
    let reading = false;
    const controller = new AbortController();
    const refresh = async () => {
      if (reading || busyRef.current) return;
      reading = true;
      setLoading(true);
      const result = await fetchGitStatus(dir, controller.signal);
      if (!cancelled) {
        if (result.ok) {
          setLoaded({ dir, repo: result.value });
          setLoadError("");
        } else {
          setLoadError(result.error);
          setLoaded(null);
        }
        setLoading(false);
      }
      reading = false;
    };
    void refresh();
    const interval = window.setInterval(() => { if (!document.hidden) void refresh(); }, 5000);
    const onFocus = () => { void refresh(); };
    window.addEventListener("focus", onFocus);
    return () => {
      cancelled = true;
      controller.abort();
      window.clearInterval(interval);
      window.removeEventListener("focus", onFocus);
    };
  }, [dir, revision]);

  const perform = async (operation: GitOperation) => {
    if (!repo || busyRef.current || loading) return;
    busyRef.current = true;
    setBusy(true);
    setNotice("");
    setError("");
    const result = await runGitOperation(repo.root, operation);
    busyRef.current = false;
    setBusy(false);
    if (currentDir.current !== dir) return;
    if (result.ok) {
      setNotice(operation.action === "commit" ? g.commitDone : g.operationDone);
      if (operation.action === "commit") setMessage("");
    } else {
      setNotice("");
      setError(result.error);
    }
    // Failed hooks or network operations can still have changed the repository.
    setRevision((value) => value + 1);
  };
  const disabled = busy || loading;
  const refresh = () => setRevision((value) => value + 1);
  const changeFiles = (files: GitEntry[], value: boolean) => {
    void perform({ action: value ? "stage" : "unstage", paths: files.map((file) => file.path) });
  };

  if (compact) {
    return (
      <aside className="session-panel git-panel git-compact" aria-label={g.title}>
        <FolderGit2 size={19} strokeWidth={1.75} aria-hidden="true" />
        <span className="git-count" title={g.fileCount(repo?.files.length ?? 0)}>{repo?.files.length ?? 0}</span>
        <button type="button" className="icon-button" title={g.refresh} aria-label={g.refresh} onClick={refresh} disabled={disabled}>
          <RotateCcw size={15} />
        </button>
        {(error || loadError) && <span role="alert" className="git-error" title={error || loadError}>!</span>}
      </aside>
    );
  }

  return (
    <aside className="session-panel git-panel" aria-label={g.title} aria-busy={busy || loading}>
      <header className="git-header">
        <FolderGit2 size={16} strokeWidth={1.75} aria-hidden="true" />
        <h2>{g.title}</h2>
        <button type="button" className="icon-button" title={g.refresh} aria-label={g.refresh} onClick={refresh} disabled={disabled}>
          <RotateCcw size={14} aria-hidden="true" />
        </button>
      </header>
      <div className="git-scroll" role="region" aria-label={g.hint} tabIndex={0}>
        <section className="git-repository" aria-label={g.repository}>
          <form className="git-folder-form" onSubmit={(event) => {
            event.preventDefault();
            if (folderInput.trim()) { setChosen(folderInput.trim()); refresh(); }
          }}>
            <input aria-label={g.repository} placeholder={g.folderPlaceholder} value={folderInput} disabled={busy}
              onChange={(event) => setFolderInput(event.target.value)} />
            <button type="submit" className="icon-button" title={g.openFolder} aria-label={g.openFolder} disabled={busy || !folderInput.trim()}>
              <FolderGit2 size={14} aria-hidden="true" />
            </button>
          </form>
          {folders.length > 0 && (
            <select className="git-folder-select" aria-label={g.chooseFolder} value={chosen ?? ""} disabled={busy}
              onChange={(event) => setChosen(event.target.value || null)}>
              <option value="">{g.follow}</option>
              {[...new Set([...folders, ...(chosen ? [chosen] : [])])].map((path) => <option key={path} value={path}>{path}</option>)}
            </select>
          )}
          {repo && <>
            <div className="git-repo-name">
              <strong title={repo.root}>{repo.root.split(/[\\/]/).filter(Boolean).pop() ?? repo.root}</strong>
              <span title={repo.root}>{repo.root}</span>
            </div>
            <div className="git-branch-row">
              <GitBranch size={13} aria-hidden="true" />
              <select aria-label={g.branch} value={repo.branch ?? ""} disabled={disabled}
                onChange={(event) => { void perform({ action: "switch", branch: event.target.value }); }}>
                {repo.branch === null && <option value="">{g.detached} · {repo.head}</option>}
                {[...new Set([...(repo.branch ? [repo.branch] : []), ...repo.branches])].map((branch) => <option key={branch} value={branch}>{branch}</option>)}
              </select>
              {repo.upstream && <span className="git-sync">
                <ArrowDown size={11} aria-hidden="true" />{repo.behind}
                <ArrowUp size={11} aria-hidden="true" />{repo.ahead}
              </span>}
            </div>
            <div className="git-remote-row">
              <span title={repo.upstream ?? g.noUpstream}>{repo.upstream ?? g.noUpstream}</span>
              <button type="button" disabled={disabled || !repo.upstream} onClick={() => { void perform({ action: "pull" }); }}>
                <ArrowDown size={12} aria-hidden="true" />{g.pull}
              </button>
              <button type="button" disabled={disabled || !repo.upstream} onClick={() => { void perform({ action: "push" }); }}>
                <ArrowUp size={12} aria-hidden="true" />{g.push}
              </button>
            </div>
          </>}
        </section>
        {(error || loadError) && <p className="git-error" role="alert">{g.error}: {error || loadError}</p>}
        {!repo && <p className="git-group-empty">{loading ? g.loading : g.chooseFolder}</p>}
        {repo && <>
          <form className="git-commit-form" onSubmit={(event) => {
            event.preventDefault();
            if (message.trim() && staged.length > 0) void perform({ action: "commit", message });
          }}>
            <textarea aria-label={g.message} placeholder={g.messagePlaceholder} value={message} rows={2} maxLength={16000}
              disabled={busy} onChange={(event) => setMessage(event.target.value)} />
            <button type="submit" className="git-commit-button"
              disabled={disabled || !message.trim() || staged.length === 0 || repo.files.some((file) => file.conflict)}>
              <Check size={14} aria-hidden="true" />{g.commit}<span>{staged.length}</span>
            </button>
          </form>
          <div className="git-change-summary"><span>{g.fileCount(repo.files.length)}</span></div>
          <FileGroup title={g.staged} files={staged} selected={selected} onSelect={setSelected} onStage={changeFiles} staged disabled={disabled} />
          <FileGroup title={g.changes} files={changes} selected={selected} onSelect={setSelected} onStage={changeFiles} staged={false} disabled={disabled} />
          {current && <DiffPreview root={repo.root} entry={current} revision={loaded} />}
          <details className="git-history" open>
            <summary><ChevronRight size={12} className="git-chevron" aria-hidden="true" /><GitCommitHorizontal size={13} aria-hidden="true" />
              {g.history}<span className="git-count">{repo.commits.length}</span>
            </summary>
            {repo.commits.length === 0 && <p className="git-group-empty">{g.emptyHistory}</p>}
            <ol>{repo.commits.map((item) => <li key={item.hash}>
              <span className="git-history-dot" aria-hidden="true" />
              <button type="button" className="git-history-select" aria-label={g.openCommit(item.message)}
                aria-expanded={selectedCommit === item.hash} aria-controls={`git-commit-${item.hash}`}
                onClick={() => setSelectedCommit((current) => current === item.hash ? null : item.hash)}>
                <span className="git-history-message" title={item.message}>{item.message}</span>
                <span className="git-history-meta"><code>{item.hash.slice(0, 7)}</code><span>{item.author}</span>
                  <time dateTime={item.time} title={item.time}>{new Date(item.time).toLocaleString(locale, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" })}</time>
                </span>
              </button>
              {selectedCommit === item.hash && <CommitDetails key={`${repo.root}:${item.hash}`} root={repo.root} hash={item.hash} />}
            </li>)}</ol>
          </details>
        </>}
      </div>
      <footer className="git-footer"><span role="status">{busy ? g.working : loading ? g.loading : notice || (repo ? g.ready : g.chooseFolder)}</span></footer>
    </aside>
  );
}

function DiffPreview({ root, entry, revision }: { root: string; entry: GitEntry; revision: unknown }) {
  const { t } = useI18n();
  const g = t.git;
  const [result, setResult] = useState<{ text: string; error: boolean } | null>(null);
  useEffect(() => {
    let cancelled = false;
    const controller = new AbortController();
    setResult(null);
    void fetchGitDiff(root, entry, controller.signal).then((result) => {
      if (!cancelled) setResult(result.ok ? { text: result.value.text, error: false } : { text: result.error, error: true });
    });
    return () => { cancelled = true; controller.abort(); };
  }, [root, entry.path, entry.staged, revision]);
  return (
    <section className="git-preview" aria-label={g.preview}>
      <div className="git-preview-heading"><FileCode2 size={12} aria-hidden="true" /><span>{g.preview}</span><span title={entry.path}>{entry.path}</span></div>
      <DiffText result={result} path={entry.path} />
    </section>
  );
}

function CommitDetails({ root, hash }: { root: string; hash: string }) {
  const { t, locale } = useI18n();
  const g = t.git;
  const [detail, setDetail] = useState<GitCommitDetail | null>(null);
  const messageRef = useRef<HTMLPreElement>(null);
  const [error, setError] = useState("");
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    const controller = new AbortController();
    void fetchGitCommit(root, hash, controller.signal).then((result) => {
      if (cancelled) return;
      if (!result.ok) { setError(result.error); return; }
      setDetail(result.value);
      setSelectedPath(result.value.files[0]?.path ?? null);
    });
    return () => { cancelled = true; controller.abort(); };
  }, [root, hash]);
  useEffect(() => {
    if (detail) messageRef.current?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [detail]);
  return (
    <section className="git-commit-detail" id={`git-commit-${hash}`} aria-label={g.commitDetails} aria-busy={!detail && !error}>
      {error ? <p className="git-error" role="alert">{error}</p> : !detail ? <p role="status">{g.loadingCommit}</p> : <>
        <pre ref={messageRef} className="git-commit-message" tabIndex={0} aria-label={g.message}>{detail.message}</pre>
        <dl className="git-commit-metadata">
          <dt>{g.commitHash}</dt><dd><code>{detail.hash}</code></dd>
          <dt>{g.commitAuthor}</dt><dd>{detail.author}<br />{detail.email}</dd>
          <dt>{g.commitDate}</dt><dd><time dateTime={detail.time}>{new Date(detail.time).toLocaleString(locale)}</time></dd>
        </dl>
        {detail.parents.length > 1 && <p className="git-merge-note">{g.firstParent} <code>{detail.parents[0].slice(0, 7)}</code></p>}
        <h3>{g.commitFiles} <span className="git-count">{detail.files.length}</span></h3>
        {detail.files.length === 0 ? <p className="git-group-empty">{g.empty}</p> :
          <div className="git-commit-files" aria-label={g.commitFiles}>{detail.files.map((file) => <button type="button" key={file.path}
            className="git-file-select" title={file.oldPath ? `${file.oldPath} → ${file.path}` : file.path}
            aria-pressed={selectedPath === file.path} aria-label={g.fileLabel(file.path, g.status[file.status] ?? file.status)}
            onClick={() => setSelectedPath(file.path)}>
            <FileText size={13} aria-hidden="true" /><span className="git-commit-file-path">{file.path}</span>
            <span className={`git-file-status status-${file.status}`} aria-hidden="true">{file.status}</span>
          </button>)}</div>}
        {selectedPath && <CommitFileDiff key={selectedPath} root={root} hash={detail.hash} path={selectedPath} />}
      </>}
    </section>
  );
}

function CommitFileDiff({ root, hash, path }: { root: string; hash: string; path: string }) {
  const { t } = useI18n();
  const [result, setResult] = useState<{ text: string; error: boolean } | null>(null);
  useEffect(() => {
    let cancelled = false;
    const controller = new AbortController();
    void fetchGitCommitDiff(root, hash, path, controller.signal).then((result) => {
      if (!cancelled) setResult(result.ok ? { text: result.value.text, error: false } : { text: result.error, error: true });
    });
    return () => { cancelled = true; controller.abort(); };
  }, [root, hash, path]);
  return <section className="git-preview" aria-label={t.git.preview}>
    <div className="git-preview-heading"><FileCode2 size={12} aria-hidden="true" /><span title={path}>{path}</span></div>
    <DiffText result={result} path={path} />
  </section>;
}

function DiffText({ result, path }: { result: { text: string; error: boolean } | null; path: string }) {
  const { t } = useI18n();
  const g = t.git;
  if (result?.error) return <p role="alert" className="git-error">{result.text}</p>;
  return <pre tabIndex={0} aria-label={g.filePreview(path)}>{result === null ? g.loading : !result.text ? g.noDiff :
    result.text.split("\n").map((line, index) => <span key={index}
      className={`git-diff-line ${line.startsWith("+") && !line.startsWith("+++") ? "added" : line.startsWith("-") && !line.startsWith("---") ? "deleted" : "context"}`}>{line}{"\n"}</span>)}</pre>;
}

interface FileGroupProps {
  title: string;
  files: GitEntry[];
  selected: Pick<GitEntry, "path" | "staged"> | null;
  staged: boolean;
  disabled: boolean;
  onSelect: (entry: GitEntry) => void;
  onStage: (files: GitEntry[], value: boolean) => void;
}

function FileGroup({ title, files, selected, staged, disabled, onSelect, onStage }: FileGroupProps) {
  const { t } = useI18n();
  const g = t.git;
  const [open, setOpen] = useState(true);
  const ActionIcon = staged ? Minus : Plus;
  return (
    <section className="git-file-group" aria-label={title}>
      <div className="git-group-heading">
        <button type="button" className="git-group-toggle" aria-expanded={open} onClick={() => setOpen(!open)}>
          {open ? <ChevronDown size={12} aria-hidden="true" /> : <ChevronRight size={12} aria-hidden="true" />}
          <span>{title}</span><span className="git-count">{files.length}</span>
        </button>
        <button type="button" className="icon-button" disabled={disabled || files.length === 0}
          aria-label={staged ? g.unstageAll : g.stageAll} title={staged ? g.unstageAll : g.stageAll} onClick={() => onStage(files, !staged)}>
          <ActionIcon size={13} aria-hidden="true" />
        </button>
      </div>
      {open && (files.length === 0 ? <p className="git-group-empty">{g.empty}</p> : <ul>{files.map((file) => {
        const slash = file.path.lastIndexOf("/");
        const status = g.status[file.status] ?? file.status;
        const selectedFile = sameGitEntry(selected, file);
        return <li key={file.path} className={`git-file-row${selectedFile ? " selected" : ""}`}>
          <button type="button" className="git-file-select" title={file.oldPath ? `${file.oldPath} → ${file.path}` : file.path}
            aria-pressed={selectedFile} aria-label={g.fileLabel(file.path, status)} onClick={() => onSelect(file)}>
            <FileText size={13} aria-hidden="true" /><span className="git-file-name">{file.path.slice(slash + 1)}</span>
            <span className="git-file-dir">{slash >= 0 ? file.path.slice(0, slash) : ""}</span>
            <span className={`git-file-status status-${file.status === "?" ? "new" : file.status}`} title={status} aria-hidden="true">{file.status}</span>
          </button>
          <button type="button" className="icon-button" disabled={disabled}
            title={staged ? g.unstage(file.path) : g.stage(file.path)} aria-label={staged ? g.unstage(file.path) : g.stage(file.path)}
            onClick={() => onStage([file], !staged)}><ActionIcon size={13} aria-hidden="true" /></button>
        </li>;
      })}</ul>)}
    </section>
  );
}
