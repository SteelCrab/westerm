import assert from "node:assert/strict";
import { test } from "node:test";
import { docsPath } from "../src/docs/api.ts";
import { createDraftStore, draftKey, type Draft } from "../src/docs/drafts.ts";
import { resolveLink } from "../src/docs/links.ts";
import { renderMarkdown, slug } from "../src/docs/markdown.ts";
import { fromDisk, toDisk, treeOf } from "../src/docs/text.ts";

// ---- links ----

test("a relative Markdown link opens from the current file's folder", () => {
  assert.deepEqual(resolveLink("README.md", "docs/usage.md"), { type: "doc", path: "docs/usage.md", hash: null });
  assert.deepEqual(resolveLink("docs/a.md", "usage.md#run"), { type: "doc", path: "docs/usage.md", hash: "run" });
  assert.deepEqual(resolveLink("docs/a.md", "../README.md"), { type: "doc", path: "README.md", hash: null });
  assert.deepEqual(resolveLink("docs/a.md", "./b.MD"), { type: "doc", path: "docs/b.MD", hash: null });
  assert.deepEqual(resolveLink("docs/deep/a.md", "/docs/usage.md"), { type: "doc", path: "docs/usage.md", hash: null }, "from the root");
  assert.deepEqual(resolveLink("a.md", "%EC%84%A4%EC%B9%98.md"), { type: "doc", path: "설치.md", hash: null });
});

test("a link that leaves the root or names no Markdown file does nothing", () => {
  assert.equal(resolveLink("README.md", "../x.md"), null);
  assert.equal(resolveLink("docs/a.md", "../../x.md"), null);
  assert.equal(resolveLink("README.md", "assets/demo.gif"), null);
  assert.equal(resolveLink("README.md", "LICENSE"), null);
});

test("web links open outside, other schemes do nothing, and # links scroll", () => {
  assert.deepEqual(resolveLink("a.md", "https://example.com/x"), { type: "external", url: "https://example.com/x" });
  assert.deepEqual(resolveLink("a.md", "http://127.0.0.1:4317"), { type: "external", url: "http://127.0.0.1:4317" });
  for (const href of ["javascript:alert(1)", "JavaScript:alert(1)", "file:///etc/passwd", "data:text/html,x", "//evil.example/a.md", "mailto:a@b.c"]) {
    assert.equal(resolveLink("a.md", href), null, href);
  }
  assert.deepEqual(resolveLink("a.md", "#the-pty-host"), { type: "anchor", id: "the-pty-host" });
  assert.equal(resolveLink("a.md", "#"), null);
});

// ---- markdown ----

test("raw HTML in a document shows as text", () => {
  const html = renderMarkdown("<script>alert(1)</script>\n\n<img src=x onerror=alert(1)>");
  assert.ok(!html.includes("<script"), html);
  assert.ok(!html.includes("<img"), html);
  assert.ok(html.includes("&lt;script&gt;"), html);
});

test("headings get ids that # links can reach", () => {
  const html = renderMarkdown("# The PTY host\n\n## `westerm` 실행\n\n## The PTY host");
  assert.ok(html.includes('<h1 id="the-pty-host">'), html);
  assert.ok(html.includes('<h2 id="westerm-실행">'), html);
  assert.ok(html.includes('<h2 id="the-pty-host-1">'), "repeated headings count up");
  assert.equal(slug("Saving & restoring: screens!"), "saving--restoring-screens");
});

test("heading ids start fresh for each document", () => {
  const source = "# Install\n\n## Install\n\n## Install";
  const expected = '<h1 id="install">Install</h1>\n<h2 id="install-1">Install</h2>\n<h2 id="install-2">Install</h2>\n';
  assert.equal(renderMarkdown(source), expected);
  assert.equal(renderMarkdown(source), expected);
});

test("plain web addresses still become links", () => {
  const html = renderMarkdown("Visit example.com or https://example.com/docs.");
  assert.ok(html.includes('<a href="http://example.com">example.com</a>'), html);
  assert.ok(html.includes('<a href="https://example.com/docs">https://example.com/docs</a>'), html);
});

test("images show their alt text only", () => {
  const html = renderMarkdown("![westerm demo](assets/demo.gif)");
  assert.ok(!html.includes("<img"), html);
  assert.ok(html.includes('<span class="md-image">westerm demo</span>'), html);
});

test("tables, code, and dangerous links render safely", () => {
  const html = renderMarkdown("| a | b |\n|---|---|\n| 1 | 2 |\n\n```bash\nnpm test\n```\n\n[x](javascript:alert(1))");
  assert.ok(html.includes("<table>") && html.includes("<td>1</td>"), html);
  assert.ok(html.includes('<code class="language-bash">npm test'), html);
  assert.ok(!html.includes('href="javascript:'), html);
});

// ---- text ----

test("a CRLF file is saved with CRLF again", () => {
  const loaded = fromDisk("a\r\nb\r\n");
  assert.deepEqual(loaded, { text: "a\nb\n", crlf: true });
  assert.equal(toDisk(loaded.text, loaded.crlf), "a\r\nb\r\n");
  assert.equal(toDisk("a\nb", false), "a\nb");
  assert.deepEqual(fromDisk("a\nb"), { text: "a\nb", crlf: false });
});

test("the tree puts files under their folders, sorted, with counts", () => {
  const tree = treeOf(["b.md", "a.md", "docs/z.md", "docs/a.md", ".github/p.md", "docs/deep/x.md"], "");
  assert.deepEqual(tree.files, ["a.md", "b.md"]);
  assert.deepEqual(tree.dirs.map((dir) => dir.path), [".github", "docs"]);
  assert.equal(tree.count, 6);
  const docs = tree.dirs[1];
  assert.deepEqual(docs.files, ["docs/a.md", "docs/z.md"]);
  assert.deepEqual(docs.dirs.map((dir) => [dir.name, dir.count]), [["deep", 1]]);
  assert.equal(docs.count, 3);
});

test("the tree filter matches paths without case", () => {
  const tree = treeOf(["README.md", "docs/usage.md", "docs/install.md"], "USA");
  assert.deepEqual(tree.files, []);
  assert.deepEqual(tree.dirs[0].files, ["docs/usage.md"]);
  assert.equal(tree.count, 1);
});

// ---- drafts ----

const draft = (text: string): Draft => ({ text, baseVersion: "v1", crlf: false });

/**
 * A storage that keeps what it is given, like localStorage across a reload.
 * One storage behind two stores is two windows of one origin. `limit` makes
 * longer values fail, like a full storage.
 */
const memoryStorage = (limit = Infinity) => {
  const items = new Map<string, string>();
  const storage = {
    writes: 0,
    /** The key of the last change, as a storage event names it. */
    changed: null as string | null,
    get length() {
      return items.size;
    },
    key: (index: number) => [...items.keys()][index] ?? null,
    getItem: (key: string) => items.get(key) ?? null,
    setItem(key: string, value: string) {
      if (value.length > limit) throw new Error("QuotaExceededError");
      items.set(key, value);
      storage.writes += 1;
      storage.changed = key;
    },
    removeItem(key: string) {
      items.delete(key);
      storage.changed = key;
    },
  };
  return storage;
};

/** Writes at once, for tests that do not look at the timing. */
const now = (task: () => void) => task();

test("every pane on a file sees one draft", () => {
  const store = createDraftStore(null);
  const key = draftKey(null, "/r", "a.md");
  let calls = 0;
  const stopA = store.subscribe(() => calls++);
  const stopB = store.subscribe(() => calls++);
  store.set(key, draft("x"));
  assert.equal(calls, 2);
  assert.deepEqual(store.get(key), draft("x"));
  store.clear(key);
  assert.equal(store.get(key), null);
  assert.equal(calls, 4);
  stopA();
  stopB();
});

test("drafts outlive a reload", () => {
  const storage = memoryStorage();
  const key = draftKey("prod", "/srv", "docs/a.md");
  createDraftStore(storage, now).set(key, draft("kept"));
  const after = createDraftStore(storage, now);
  assert.deepEqual(after.get(key), draft("kept"));
  assert.deepEqual(after.paths("prod", "/srv"), new Set(["docs/a.md"]));
  assert.deepEqual(after.paths(null, "/srv"), new Set(), "a local window has its own drafts");
});

test("drafts of two windows both outlive a reload", () => {
  const storage = memoryStorage();
  const local = createDraftStore(storage, now);
  const remote = createDraftStore(storage, now);
  const x = draftKey(null, "/r", "x.md");
  const y = draftKey("prod", "/srv", "y.md");
  local.set(x, draft("x"));
  remote.set(y, draft("y"));
  const reopened = createDraftStore(storage, now);
  assert.deepEqual(reopened.get(x), draft("x"), "the remote window did not erase the local draft");
  assert.deepEqual(reopened.get(y), draft("y"));
});

test("a draft changed in another window shows here", () => {
  const storage = memoryStorage();
  const other = createDraftStore(storage, now);
  const here = createDraftStore(storage, now);
  const key = draftKey(null, "/r", "a.md");
  let calls = 0;
  here.subscribe(() => calls++);
  other.set(key, draft("theirs"));
  here.sync(storage.changed);
  assert.deepEqual(here.get(key), draft("theirs"));
  assert.equal(calls, 1);
  other.clear(key);
  here.sync(storage.changed);
  assert.equal(here.get(key), null);
  here.sync("someone.else's.key");
  assert.equal(calls, 2, "other keys change nothing");
  other.set(key, draft("again"));
  here.sync(null); // another window cleared the storage, or many keys changed
  assert.deepEqual(here.get(key), draft("again"));
});

test("a draft is written once typing pauses, and at once when the page hides", () => {
  const tasks: (() => void)[] = [];
  const storage = memoryStorage();
  const store = createDraftStore(storage, (task) => void tasks.push(task));
  const key = draftKey(null, "/r", "a.md");
  store.set(key, draft("a"));
  store.set(key, draft("ab"));
  assert.deepEqual(store.get(key), draft("ab"), "the panes see each key at once");
  assert.equal(storage.writes, 0);
  assert.equal(tasks.length, 1);
  tasks.shift()?.();
  assert.equal(storage.writes, 1);
  assert.deepEqual(createDraftStore(storage, now).get(key), draft("ab"));
  store.set(key, draft("abc"));
  store.flush();
  assert.deepEqual(createDraftStore(storage, now).get(key), draft("abc"));
  const gone = draftKey(null, "/r", "gone.md");
  store.set(gone, draft("x"));
  store.clear(gone);
  for (const task of tasks.splice(0)) task();
  assert.equal(createDraftStore(storage, now).get(gone), null, "a cleared draft is not written late");
});

test("a draft that does not fit is marked unkept and leaves no old copy", () => {
  const storage = memoryStorage(60);
  const store = createDraftStore(storage, now);
  const key = draftKey(null, "/r", "a.md");
  let calls = 0;
  store.subscribe(() => calls++);
  store.set(key, draft("short"));
  assert.equal(store.unkept(key), false);
  store.set(key, draft("a much longer text that does not fit"));
  assert.equal(store.unkept(key), true);
  assert.deepEqual(store.get(key), draft("a much longer text that does not fit"), "this window keeps it");
  assert.equal(createDraftStore(storage, now).get(key), null, "a reload does not bring back the old text");
  assert.ok(calls >= 2);
  store.set(key, draft("short"));
  assert.equal(store.unkept(key), false);
});

test("drafts saved by the first version move to one key each", () => {
  const storage = memoryStorage();
  const key = draftKey(null, "/r", "a.md");
  storage.setItem("westerm.docs.drafts", JSON.stringify({ [key]: draft("old"), junk: { text: 1 } }));
  const store = createDraftStore(storage, now);
  assert.deepEqual(store.get(key), draft("old"));
  assert.equal(storage.getItem("westerm.docs.drafts"), null);
  assert.deepEqual(createDraftStore(storage, now).get(key), draft("old"));
});

test("a storage that throws or holds junk costs only the persistence", () => {
  const fail = () => {
    throw new Error("denied");
  };
  const throwing = { length: 1, key: fail, getItem: fail, setItem: fail, removeItem: fail };
  const store = createDraftStore(throwing, now);
  store.set("k", draft("x"));
  assert.deepEqual(store.get("k"), draft("x"));
  const storage = memoryStorage();
  const key = draftKey(null, "/r", "a.md");
  createDraftStore(storage, now).set(key, draft("x"));
  const stored = storage.changed ?? "";
  storage.setItem(stored, "{not json");
  assert.equal(createDraftStore(storage, now).get(key), null);
});

// ---- api ----

test("docs requests of a remote window go to its server", () => {
  assert.equal(docsPath("tree", { root: "/r" }, null), "/api/docs/tree?root=%2Fr");
  assert.equal(docsPath("tree", { root: "/r" }, "prod"), "/remote/prod/api/docs/tree?root=%2Fr");
  assert.equal(docsPath("file", undefined, "me@host"), "/remote/me%40host/api/docs/file");
});

test("drafts in other folders of the same machine are listed", () => {
  const store = createDraftStore(null);
  store.set(draftKey(null, "/a", "x.md"), draft("1"));
  store.set(draftKey(null, "/b", "docs/y.md"), draft("2"));
  store.set(draftKey(null, "/b", "z.md"), draft("3"));
  store.set(draftKey("prod", "/c", "w.md"), draft("4"));
  assert.deepEqual(store.elsewhere(null, "/a"), [
    { root: "/b", path: "docs/y.md" },
    { root: "/b", path: "z.md" },
  ]);
  assert.deepEqual(store.elsewhere("prod", "/a"), [{ root: "/c", path: "w.md" }]);
});

// ---- recent roots ----

import { createRecentRoots } from "../src/docs/recent.ts";

test("recent roots list the newest first, once each, at most eight", () => {
  const storage = memoryStorage();
  const recent = createRecentRoots(storage);
  for (const root of ["/1", "/2", "/3", "/4", "/5", "/6", "/7", "/8", "/9", "/3"]) recent.remember(root);
  assert.deepEqual(recent.list(), ["/3", "/9", "/8", "/7", "/6", "/5", "/4", "/2"]);
  assert.deepEqual(createRecentRoots(storage).list(), recent.list(), "kept across a reload");
  assert.deepEqual(createRecentRoots({ getItem: () => "[1, null]", setItem: () => {} }).list(), [], "junk is ignored");
});
