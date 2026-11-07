// The docs pane: a folder's Markdown files as a tree, and the open file in
// view, edit, or split mode. Unsaved text lives in the shared draft store
// (drafts.ts), so every pane on a file shows the same edit and nothing is
// lost when a pane or the page closes.

import { useCallback, useDeferredValue, useEffect, useMemo, useRef, useState } from "react";
import {
  ChevronRight,
  Columns2,
  Eye,
  FilePlus,
  FileText,
  Folder,
  FolderOpen,
  ListTree,
  Pencil,
  Search,
} from "lucide-react";
import { isDocPath, type DocsMode } from "../../shared/workspace-file.ts";
import { useI18n } from "../i18n/index.tsx";
import type { Messages } from "../i18n/messages.ts";
import { IS_MAC } from "../paletteCommands.tsx";
import { REMOTE_TARGET } from "../remoteWindow.ts";
import { useNarrow } from "../useNarrow.ts";
import type { DashboardAction, DocsPane, Session } from "../workspace.ts";
import { createFile, fetchCwd, fetchFile, fetchTree, saveFile, type Failed } from "./api.ts";
import { draftKey, drafts, pageStorage, useDrafts } from "./drafts.ts";
import { resolveLink } from "./links.ts";
import { renderMarkdown } from "./markdown.ts";
import { createRecentRoots } from "./recent.ts";
import { fromDisk, toDisk, treeOf, type TreeNode } from "./text.ts";

/** How often a visible pane reads the tree and the open file again. */
const REFRESH_MS = 5000;
/** How often a pane that follows a shell asks for that shell's folder. */
const FOLLOW_MS = 3000;
/** Must match MAX_FILES in native/src/server/docs.rs. */
const MAX_FILES = 2000;

/** Folders this window's machine showed lately, for the root field's list. */
const recentRoots = createRecentRoots(pageStorage(), REMOTE_TARGET);

interface DocsViewProps {
  session: Session;
  focused: boolean;
  visible: boolean;
  /** Changes when the dashboard hands keyboard focus back to the pane. */
  focusNonce: number;
  /** The followed shell's pid while it runs. */
  followPid: number | null;
  /** The followed shell still exists (it may be reconnecting). */
  followAlive: boolean;
  /** A running shell of this tab the pane may start to follow. */
  followCandidate: string | null;
  dispatch: (action: DashboardAction) => void;
  onOpenUrl: (url: string) => void;
}

/** The file as read from disk, with `\n` line ends. */
interface Loaded {
  path: string;
  text: string;
  crlf: boolean;
  version: string;
}

type ErrorText = Messages["docs"]["error"];

function failure(error: ErrorText, result: Failed, missing: keyof ErrorText = "missing"): string {
  if (result.status === 404) return error[missing] as string;
  if (result.status === 413) return error.tooLarge;
  if (result.status === 415) return error.notText;
  return error.failed(result.status);
}

function baseName(path: string): string {
  return path.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || path;
}

/** Every folder that holds a file, for the count under the tree. */
function folderCount(files: readonly string[]): number {
  return new Set(files.map((file) => file.split("/").slice(0, -1).join("/"))).size;
}

export function DocsView(props: DocsViewProps) {
  const { session, focused, visible, followPid, followAlive, followCandidate, dispatch, onOpenUrl } = props;
  const docs = session.docs as DocsPane;
  const { root, file, mode, followFrom } = docs;
  const id = session.id;
  const { t } = useI18n();
  const narrow = useNarrow();
  const update = useCallback((change: Partial<DocsPane>) => dispatch({ type: "docsUpdate", id, change }), [dispatch, id]);

  const [tree, setTree] = useState<{ files: string[]; truncated: boolean } | null>(null);
  const [treeError, setTreeError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const [fileError, setFileError] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [newName, setNewName] = useState<string | null>(null);
  const [newError, setNewError] = useState<string | null>(null);
  const [rootInput, setRootInput] = useState<string | null>(null);
  const [rootError, setRootError] = useState<string | null>(null);
  /** The disk's version after a save was refused; null when the file is gone. */
  const [conflict, setConflict] = useState<{ version: string | null } | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [treeOpen, setTreeOpen] = useState(false);
  /** Why the pane has no folder: the server has no docs routes, or did not answer. */
  const [resolveError, setResolveError] = useState<string | null>(null);
  const [elsewhereOpen, setElsewhereOpen] = useState(false);

  const rootRef = useRef<HTMLDivElement>(null);
  const previewRef = useRef<HTMLElement>(null);
  const editorRef = useRef<HTMLTextAreaElement>(null);
  const gutterRef = useRef<HTMLDivElement>(null);
  /** A heading to scroll to once the linked file shows. */
  const pendingHash = useRef<string | null>(null);

  useDrafts();
  const key = root && file ? draftKey(REMOTE_TARGET, root, file) : null;
  const draft = key ? drafts.get(key) : null;
  const draftPaths = root ? drafts.paths(REMOTE_TARGET, root) : new Set<string>();
  const elsewhere = root ? drafts.elsewhere(REMOTE_TARGET, root) : [];
  const current = loaded && loaded.path === file ? loaded : null;
  const text = draft?.text ?? current?.text ?? "";
  const shownMode: DocsMode = narrow && mode === "split" ? "edit" : mode;

  // A new pane asks for its folder: the followed shell's, else home.
  useEffect(() => {
    if (root !== null) return;
    let cancelled = false;
    void (async () => {
      const followed = followPid !== null ? await fetchCwd(followPid) : null;
      const found = followed?.ok ? followed : await fetchCwd(null);
      if (cancelled) return;
      if (found.ok) {
        setResolveError(null);
        update({ root: found.path });
      } else {
        // A server from before docs panes answers 404 for every docs route.
        setResolveError(found.status === 404 ? t.docs.error.unsupported : t.docs.error.failed(found.status));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [root, followPid, update, t]);

  // The pane is named after its folder, like a shell after its title.
  useEffect(() => {
    if (!root) return;
    dispatch({ type: "title", id, title: baseName(root) });
    recentRoots.remember(root);
  }, [root, id, dispatch]);

  const loadTree = useCallback(async () => {
    if (!root) return;
    const result = await fetchTree(root);
    if (result.ok) {
      setTree({ files: result.files, truncated: result.truncated });
      setTreeError(null);
    } else {
      setTree(null);
      const reason = result.status === 400 ? "badRoot" : result.status === 404 ? "unsupported" : null;
      setTreeError(reason ? t.docs.error[reason] : t.docs.error.failed(result.status));
    }
  }, [root, t]);

  const loadFile = useCallback(async () => {
    if (!root || !file) return;
    const result = await fetchFile(root, file);
    if (result.ok) {
      const disk = fromDisk(result.content);
      setLoaded((previous) =>
        previous && previous.path === file && previous.version === result.version
          ? previous
          : { path: file, text: disk.text, crlf: disk.crlf, version: result.version },
      );
      setFileError(null);
    } else {
      setFileError(failure(t.docs.error, result));
    }
  }, [root, file, t]);

  // A new root or file starts clean.
  useEffect(() => {
    setTree(null);
    setTreeError(null);
    void loadTree();
  }, [loadTree]);
  useEffect(() => {
    setLoaded(null);
    setFileError(null);
    setConflict(null);
    setSaveError(null);
    void loadFile();
    if (previewRef.current) previewRef.current.scrollTop = 0;
    if (editorRef.current) editorRef.current.scrollTop = 0;
  }, [loadFile]);

  // While visible: read the tree and the open file again. An edited file is
  // left alone; a save finds out whether the disk changed.
  const hasDraft = draft !== null;
  useEffect(() => {
    if (!visible) return;
    const refresh = () => {
      void loadTree();
      if (!hasDraft) void loadFile();
    };
    if (focused) refresh();
    const timer = setInterval(refresh, REFRESH_MS);
    return () => clearInterval(timer);
  }, [visible, focused, hasDraft, loadTree, loadFile]);

  // Following a shell: its folder becomes the root. Drafts stay in the store.
  useEffect(() => {
    if (followFrom === null) return;
    if (!followAlive) {
      update({ followFrom: null });
      return;
    }
    if (!visible || followPid === null) return;
    let cancelled = false;
    const check = async () => {
      const result = await fetchCwd(followPid);
      if (!cancelled && result.ok && result.path !== root) update({ root: result.path, file: null });
    };
    void check();
    const timer = setInterval(() => void check(), FOLLOW_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [followFrom, followAlive, followPid, visible, root, update]);

  // Keys go to the pane when the dashboard focuses it.
  useEffect(() => {
    if (focused && visible && rootRef.current && !rootRef.current.contains(document.activeElement)) {
      rootRef.current.focus({ preventScroll: true });
    }
  }, [focused, visible, props.focusNonce]);

  const save = useCallback(
    async (baseOverride?: string | null) => {
      if (!root || !file || !key) return;
      const pending = drafts.get(key);
      if (!pending || saving) return;
      const base = baseOverride !== undefined ? baseOverride : pending.baseVersion;
      setSaving(true);
      setSaveError(null);
      const result = await saveFile(root, file, toDisk(pending.text, pending.crlf), base);
      setSaving(false);
      if (result.ok) {
        setConflict(null);
        setLoaded({ path: file, text: pending.text, crlf: pending.crlf, version: result.version });
        // Typing during the save keeps a draft, now based on the saved file.
        const now = drafts.get(key);
        if (now && now.text !== pending.text) drafts.set(key, { ...now, baseVersion: result.version });
        else drafts.clear(key);
        void loadTree();
      } else if (result.status === 409) {
        setConflict({ version: result.version ?? null });
      } else {
        setSaveError(failure(t.docs.error, result));
      }
    },
    [root, file, key, saving, t, loadTree],
  );

  const discard = () => {
    if (key) drafts.clear(key);
    setConflict(null);
    void loadFile();
  };

  const edit = (value: string) => {
    if (!key || !current) return;
    const base = draft ?? { baseVersion: current.version, crlf: current.crlf };
    // Typing the file back to what is on disk leaves no draft.
    if (value === current.text && base.baseVersion === current.version) drafts.clear(key);
    else drafts.set(key, { text: value, baseVersion: base.baseVersion, crlf: base.crlf });
  };

  const openFile = (path: string) => {
    update({ file: path });
    if (narrow) setTreeOpen(false);
  };

  const createDoc = async () => {
    if (!root || newName === null) return;
    const name = newName.trim();
    if (!name) {
      setNewName(null);
      return;
    }
    const path = /\.(md|markdown)$/i.test(name) ? name : `${name}.md`;
    if (!isDocPath(path)) {
      setNewError(t.docs.error.badName);
      return;
    }
    const title = baseName(path).replace(/\.(md|markdown)$/i, "");
    const result = await createFile(root, path, `# ${title}\n`);
    if (!result.ok) {
      setNewError(result.status === 409 ? t.docs.error.exists : result.status === 400 ? t.docs.error.badName : t.docs.error.failed(result.status));
      return;
    }
    setNewName(null);
    setNewError(null);
    await loadTree();
    update({ file: path, mode: "split" });
  };

  const changeRoot = async () => {
    if (rootInput === null) return;
    const next = rootInput.trim();
    if (!next || next === root) {
      setRootInput(null);
      return;
    }
    const result = await fetchTree(next);
    if (!result.ok) {
      setRootError(t.docs.error.badRoot);
      return;
    }
    setRootInput(null);
    setRootError(null);
    // The server's spelling of the folder, so `/a/b/` and `/a/b` are one root.
    update({ root: result.root, followFrom: null, file: null });
  };

  const toggleFollow = () => {
    if (followFrom !== null) update({ followFrom: null });
    else if (followCandidate !== null) update({ followFrom: followCandidate });
  };

  // Links: Markdown files open here, web pages in a browser pane, `#` scrolls.
  const followLink = (event: React.MouseEvent) => {
    const anchor = (event.target as HTMLElement).closest("a");
    if (!anchor) return;
    event.preventDefault();
    const target = resolveLink(file ?? "", anchor.getAttribute("href") ?? "");
    if (!target) return;
    if (target.type === "external") onOpenUrl(target.url);
    else if (target.type === "anchor") scrollToHeading(target.id);
    else if (target.path === file) {
      if (target.hash) scrollToHeading(target.hash);
    } else {
      pendingHash.current = target.hash;
      update({ file: target.path });
    }
  };

  const scrollToHeading = (headingId: string) => {
    const preview = previewRef.current;
    const heading = preview?.querySelector(`[id="${CSS.escape(headingId)}"]`);
    heading?.scrollIntoView({ block: "start" });
  };

  const deferredText = useDeferredValue(text);
  const html = useMemo(() => (current || draft ? renderMarkdown(deferredText) : ""), [deferredText, current, draft]);
  useEffect(() => {
    if (!pendingHash.current || !current) return;
    scrollToHeading(pendingHash.current);
    pendingHash.current = null;
  }, [html, current]);

  const onKeyDown = (event: React.KeyboardEvent) => {
    const mod = IS_MAC ? event.metaKey : event.ctrlKey;
    if (!mod || event.altKey || event.shiftKey) return;
    if (event.code === "KeyS") {
      event.preventDefault();
      void save();
    } else if (event.code === "KeyE") {
      event.preventDefault();
      update({ mode: mode === "view" ? "split" : "view" });
    }
  };

  const lineCount = text.split("\n").length;
  const gutter = useMemo(() => Array.from({ length: lineCount }, (_, index) => index + 1).join("\n"), [lineCount]);
  const nodes = useMemo(() => (tree ? treeOf(tree.files, filter) : null), [tree, filter]);
  const following = followFrom !== null;
  const modKey = IS_MAC ? "⌘" : "Ctrl+";

  return (
    <div
      ref={rootRef}
      className={`docs-view${narrow ? " narrow" : ""}${narrow && treeOpen ? " tree-open" : ""}`}
      tabIndex={-1}
      onKeyDown={onKeyDown}
      onFocus={() => !focused && dispatch({ type: "focus", id })}
    >
      <div className="docs-toolbar">
        {rootInput === null ? (
          <button type="button" className="docs-root" title={`${t.docs.root}: ${root ?? ""} — ${t.docs.rootHint}`} onClick={() => setRootInput(root ?? "")}>
            <Folder size={13} aria-hidden="true" />
            {/* Marks keep a path's slashes in place in the right-to-left box that trims its start. */}
            <span className={resolveError ? "docs-error" : undefined}>{root ? `\u200e${root}\u200e` : (resolveError ?? t.docs.loading)}</span>
          </button>
        ) : (
          <form
            className="docs-root-form"
            onSubmit={(event) => {
              event.preventDefault();
              void changeRoot();
            }}
          >
            <input
              autoFocus
              aria-label={t.docs.root}
              list={`${id}-recent-roots`}
              value={rootInput}
              placeholder={t.docs.rootPlaceholder}
              onChange={(event) => {
                setRootInput(event.target.value);
                setRootError(null);
              }}
              onKeyDown={(event) => event.key === "Escape" && (setRootInput(null), setRootError(null))}
              onBlur={() => !rootError && setRootInput(null)}
            />
            <datalist id={`${id}-recent-roots`}>
              {recentRoots.list().map((recent) => (
                <option key={recent} value={recent} />
              ))}
            </datalist>
            {rootError && <span className="docs-error" role="alert">{rootError}</span>}
          </form>
        )}
        <button
          type="button"
          className={`docs-follow${following ? " on" : ""}`}
          aria-pressed={following}
          disabled={!following && followCandidate === null}
          title={following || followCandidate !== null ? t.docs.followHint : t.docs.followNone}
          onClick={toggleFollow}
        >
          <span className="docs-switch" aria-hidden="true" />
          {t.docs.follow}
        </button>
      </div>

      <div className="docs-main">
        <aside className="docs-tree" aria-label={t.kinds.docs.name}>
          <div className="docs-tree-top">
            <label className="docs-search">
              <Search size={13} aria-hidden="true" />
              <input value={filter} placeholder={t.docs.search} aria-label={t.docs.search} onChange={(event) => setFilter(event.target.value)} />
            </label>
            <button type="button" className="icon-button" title={t.docs.newDoc} aria-label={t.docs.newDoc} disabled={!root} onClick={() => setNewName("")}>
              <FilePlus size={14} />
            </button>
          </div>
          {newName !== null && (
            <form
              className="docs-new"
              onSubmit={(event) => {
                event.preventDefault();
                void createDoc();
              }}
            >
              <input
                autoFocus
                aria-label={t.docs.newDoc}
                value={newName}
                placeholder={t.docs.newDocPlaceholder}
                onChange={(event) => {
                  setNewName(event.target.value);
                  setNewError(null);
                }}
                onKeyDown={(event) => event.key === "Escape" && (setNewName(null), setNewError(null))}
              />
              {newError && <span className="docs-error" role="alert">{newError}</span>}
            </form>
          )}
          <div className="docs-tree-list" role="tree">
            {treeError ? (
              <p className="docs-note docs-error">{treeError}</p>
            ) : !nodes ? (
              <p className="docs-note">{t.docs.loading}</p>
            ) : nodes.count === 0 ? (
              <p className="docs-note">{filter ? t.docs.noMatch : t.docs.empty}</p>
            ) : (
              <TreeRows node={nodes} depth={0} current={file} dirty={draftPaths} forceOpen={filter.trim() !== ""} onOpen={openFile} />
            )}
          </div>
          {elsewhere.length > 0 && (
            <div className="docs-elsewhere">
              <button type="button" className="docs-link-button" aria-expanded={elsewhereOpen} onClick={() => setElsewhereOpen(!elsewhereOpen)}>
                <span className="docs-dirty" aria-hidden="true" />
                {t.docs.elsewhere(elsewhere.length)}
              </button>
              {elsewhereOpen &&
                elsewhere.map((item) => (
                  <button
                    key={`${item.root}/${item.path}`}
                    type="button"
                    className="docs-elsewhere-item"
                    title={`${item.root}/${item.path}`}
                    onClick={() => {
                      setElsewhereOpen(false);
                      update({ root: item.root, file: item.path, followFrom: null });
                    }}
                  >
                    <span>{baseName(item.root)}</span> / {item.path}
                  </button>
                ))}
            </div>
          )}
          {tree && (
            <div className="docs-tree-foot">
              {t.docs.count(tree.files.length, folderCount(tree.files))}
              {tree.truncated && <div>{t.docs.truncated(MAX_FILES)}</div>}
            </div>
          )}
        </aside>

        <section className="docs-doc">
          <div className="docs-bar">
            {narrow && (
              <button type="button" className="icon-button" aria-pressed={treeOpen} title={treeOpen ? t.docs.hideTree : t.docs.showTree} aria-label={treeOpen ? t.docs.hideTree : t.docs.showTree} onClick={() => setTreeOpen(!treeOpen)}>
                <ListTree size={14} />
              </button>
            )}
            <div className="docs-crumb" title={file ?? undefined}>
              {file?.split("/").map((part, index, parts) =>
                index === parts.length - 1 ? <b key={index}>{part}</b> : <span key={index}>{part} / </span>,
              )}
            </div>
            {file && (
              <span className={`docs-state${draft ? " dirty" : ""}`}>{saving ? t.docs.saving : draft ? `● ${t.docs.unsaved}` : current ? t.docs.saved : ""}</span>
            )}
            {draft && (
              <button type="button" className="docs-link-button" title={t.docs.discardHint} onClick={discard}>
                {t.docs.discard}
              </button>
            )}
            <span className="pane-spacer" />
            <div className="docs-modes" role="group">
              <ModeButton mode="view" shown={shownMode} label={t.docs.view} icon={<Eye size={13} />} onPick={(next) => update({ mode: next })} />
              <ModeButton mode="edit" shown={shownMode} label={t.docs.edit} icon={<Pencil size={13} />} onPick={(next) => update({ mode: next })} />
              {!narrow && <ModeButton mode="split" shown={shownMode} label={t.docs.split} icon={<Columns2 size={13} />} onPick={(next) => update({ mode: next })} />}
            </div>
            <button type="button" className="button primary docs-save" disabled={!draft || saving} onClick={() => void save()}>
              {t.docs.save}
            </button>
          </div>

          {conflict && (
            <div className="docs-conflict" role="alert">
              <span>{t.docs.conflict}</span>
              <button type="button" className="button" onClick={discard}>
                {t.docs.reload}
              </button>
              <button type="button" className="button" onClick={() => void save(conflict.version)}>
                {t.docs.overwrite}
              </button>
            </div>
          )}
          {saveError && (
            <div className="docs-conflict" role="alert">
              <span>{saveError}</span>
            </div>
          )}

          {!file ? (
            <p className="docs-empty">{t.docs.pick}</p>
          ) : fileError && !draft ? (
            <p className="docs-empty docs-error">{fileError}</p>
          ) : !current && !draft ? (
            <p className="docs-empty">{t.docs.loading}</p>
          ) : (
            <div className={`docs-body ${shownMode}`}>
              {shownMode !== "view" && (
                <div className="docs-editor">
                  <div ref={gutterRef} className="docs-gutter" aria-hidden="true">
                    {gutter}
                  </div>
                  <textarea
                    ref={editorRef}
                    value={text}
                    spellCheck={false}
                    aria-label={file}
                    onChange={(event) => edit(event.target.value)}
                    onScroll={(event) => {
                      if (gutterRef.current) gutterRef.current.scrollTop = event.currentTarget.scrollTop;
                    }}
                  />
                </div>
              )}
              {shownMode !== "edit" && (
                <article
                  ref={previewRef}
                  className="docs-preview md"
                  onClick={followLink}
                  onAuxClick={(event) => (event.target as HTMLElement).closest("a") && event.preventDefault()}
                  dangerouslySetInnerHTML={{ __html: html }}
                />
              )}
            </div>
          )}

          <div className="docs-status">
            {file && <span>{t.docs.lines(lineCount)}</span>}
            {file && <span>{draft?.crlf ?? current?.crlf ? "CRLF" : "LF"}</span>}
            {key && drafts.unkept(key) && (
              <span className="docs-unkept" role="status" title={t.docs.unkept}>
                {t.docs.unkept}
              </span>
            )}
            <span className="pane-spacer" />
            <span className="docs-keys">{t.docs.keys(modKey)}</span>
          </div>
        </section>
      </div>
    </div>
  );
}

function ModeButton(props: { mode: DocsMode; shown: DocsMode; label: string; icon: React.ReactNode; onPick: (mode: DocsMode) => void }) {
  return (
    <button type="button" className={props.shown === props.mode ? "on" : undefined} aria-pressed={props.shown === props.mode} onClick={() => props.onPick(props.mode)}>
      {props.icon}
      {props.label}
    </button>
  );
}

interface TreeRowsProps {
  node: TreeNode;
  depth: number;
  current: string | null;
  /** Paths with an unsaved draft. */
  dirty: ReadonlySet<string>;
  /** While filtering every folder shows open. */
  forceOpen: boolean;
  onOpen: (path: string) => void;
}

/** A folder's rows; the root shows only its content. Folders start open down to the open file. */
function TreeRows(props: TreeRowsProps) {
  const { node, depth, current, dirty, forceOpen, onOpen } = props;
  return (
    <>
      {node.dirs.map((dir) => (
        <FolderRow key={dir.path} {...props} node={dir} />
      ))}
      {node.files.map((path) => (
        <div
          key={path}
          role="treeitem"
          aria-selected={path === current}
          tabIndex={0}
          className={`docs-row file${path === current ? " on" : ""}`}
          style={{ paddingLeft: 10 + depth * 14 + 16 }}
          title={path}
          onClick={() => onOpen(path)}
          onKeyDown={(event) => event.key === "Enter" && onOpen(path)}
        >
          <FileText size={13} aria-hidden="true" />
          <span className="docs-row-name">{path.split("/").pop()}</span>
          {dirty.has(path) && <span className="docs-dirty" aria-hidden="true" />}
        </div>
      ))}
    </>
  );
}

function FolderRow(props: TreeRowsProps) {
  const { node, depth, current, forceOpen } = props;
  const holdsCurrent = current !== null && current.startsWith(`${node.path}/`);
  const [open, setOpen] = useState(holdsCurrent || node.count <= 12);
  const shown = forceOpen || open;
  return (
    <>
      <div
        role="treeitem"
        aria-expanded={shown}
        tabIndex={0}
        className="docs-row folder"
        style={{ paddingLeft: 10 + depth * 14 }}
        onClick={() => setOpen(!shown)}
        onKeyDown={(event) => event.key === "Enter" && setOpen(!shown)}
      >
        <ChevronRight size={12} className={`docs-chevron${shown ? " open" : ""}`} aria-hidden="true" />
        {shown ? <FolderOpen size={13} aria-hidden="true" /> : <Folder size={13} aria-hidden="true" />}
        <span className="docs-row-name">{node.name}</span>
        <span className="docs-count">{node.count}</span>
      </div>
      {shown && <TreeRows {...props} depth={depth + 1} />}
    </>
  );
}
