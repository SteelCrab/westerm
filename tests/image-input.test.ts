import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { Window } from "happy-dom";
import type { WTerm } from "@wterm/dom";
import {
  guardFileDrops,
  imagesPath,
  installImageInput,
  MAX_IMAGE_BYTES,
  pastedImages,
  shellPath,
  uploadImage,
  type ImageNotice,
} from "../src/terminal/image-input.ts";

const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3]);
const png = (name = "image.png") => new File([PNG], name, { type: "image/png" });

/** What a paste or drop event carries. */
function transfer(files: File[], text = ""): DataTransfer {
  return {
    files,
    items: [],
    types: files.length > 0 ? ["Files"] : [],
    dropEffect: "none",
    getData: (type: string) => (type === "text" || type === "text/plain" ? text : ""),
  } as unknown as DataTransfer;
}

const originalFetch = globalThis.fetch;
afterEach(() => {
  globalThis.fetch = originalFetch;
});

test("a remote window sends pictures to its server", () => {
  assert.equal(imagesPath(null), "/api/images");
  assert.equal(imagesPath("user@server"), "/remote/user%40server/api/images");
});

test("a path is pasted the way a native terminal pastes a dropped file", () => {
  assert.equal(shellPath("/home/me/.local/state/westerm/images/image-1-ab.png"), "/home/me/.local/state/westerm/images/image-1-ab.png");
  assert.equal(shellPath("/Users/my name/it's (1).png"), "/Users/my\\ name/it\\'s\\ \\(1\\).png");
  assert.equal(shellPath("C:\\Users\\me\\AppData\\Local\\westerm\\images\\a.png"), "C:\\Users\\me\\AppData\\Local\\westerm\\images\\a.png");
  assert.equal(shellPath("C:\\Users\\my name\\a.png"), '"C:\\Users\\my name\\a.png"');
});

test("a paste is a picture only when the clipboard has no text of its own", () => {
  assert.deepEqual(pastedImages(transfer([png()])).map((file) => file.name), ["image.png"]);
  // A copied spreadsheet cell: its text and a picture of it. The text is the paste.
  assert.deepEqual(pastedImages(transfer([png()], "42")), []);
  // A file copied in the file manager: the text is only its name.
  assert.deepEqual(pastedImages(transfer([png("shot.png")], "shot.png")).map((file) => file.name), ["shot.png"]);
  assert.deepEqual(pastedImages(transfer([new File(["x"], "notes.txt", { type: "text/plain" })])), []);
  assert.deepEqual(pastedImages(transfer([])), []);
  assert.deepEqual(pastedImages(null), []);
});

test("a picture in the items list is found when the files list is empty", () => {
  const data = {
    files: [],
    items: [{ kind: "string", type: "text/html", getAsFile: () => null }, { kind: "file", type: "image/png", getAsFile: () => png() }],
    types: ["Files"],
    getData: () => "",
  } as unknown as DataTransfer;
  assert.deepEqual(pastedImages(data).map((file) => file.name), ["image.png"]);
});

test("an upload sends the picture as base64 JSON and returns the server's path", async () => {
  let seen: { url: string; init: RequestInit | undefined } | null = null;
  globalThis.fetch = async (url, init) => {
    seen = { url: String(url), init };
    return new Response(JSON.stringify({ path: "/state/images/image-1-ab.png" }), { status: 200 });
  };
  assert.deepEqual(await uploadImage(png(), null), { ok: true, path: "/state/images/image-1-ab.png" });
  assert.ok(seen);
  const { url, init } = seen as { url: string; init: RequestInit };
  assert.equal(url, "/api/images");
  assert.equal(init.method, "POST");
  assert.equal(new Headers(init.headers).get("content-type"), "application/json");
  assert.deepEqual(JSON.parse(String(init.body)), { data: Buffer.from(PNG).toString("base64") });
});

test("an upload reports a picture that is too large, and any other failure", async () => {
  let calls = 0;
  globalThis.fetch = async () => {
    calls++;
    return new Response("", { status: 413 });
  };
  const large = { size: MAX_IMAGE_BYTES + 1, arrayBuffer: async () => new ArrayBuffer(0) } as unknown as File;
  assert.deepEqual(await uploadImage(large, null), { ok: false, reason: "tooLarge" });
  assert.equal(calls, 0, "the server is not asked");
  assert.deepEqual(await uploadImage(png(), null), { ok: false, reason: "tooLarge" });

  globalThis.fetch = async () => new Response("", { status: 415 });
  assert.deepEqual(await uploadImage(png(), null), { ok: false, reason: "notImage" });
  globalThis.fetch = async () => new Response("{}", { status: 200 });
  assert.deepEqual(await uploadImage(png(), null), { ok: false, reason: "failed" }, "no path in the answer");
  globalThis.fetch = async () => {
    throw new TypeError("network");
  };
  assert.deepEqual(await uploadImage(png(), null), { ok: false, reason: "failed" });
});

/** A terminal element with the input installed; `settled` waits for the uploads. */
function terminal(enabled = true) {
  const browser = new Window({ url: "http://127.0.0.1/" });
  const element = browser.document.createElement("div");
  const textarea = element.appendChild(browser.document.createElement("textarea"));
  const pasted: string[] = [];
  const notices: (ImageNotice | null)[] = [];
  let textPastes = 0;
  // wterm's own paste listener, on the textarea inside the element.
  textarea.addEventListener("paste", () => textPastes++);
  let focused = 0;
  const wt = { element, focus: () => focused++ } as unknown as WTerm;
  const uninstall = installImageInput(wt, {
    enabled,
    paste: (text) => pasted.push(text),
    notify: (notice) => notices.push(notice),
  });
  const fire = (type: string, field: "clipboardData" | "dataTransfer", data: DataTransfer, extra: Record<string, unknown> = {}) => {
    const event = new browser.Event(type, { bubbles: true, cancelable: true });
    Object.defineProperty(event, field, { value: data });
    for (const [name, value] of Object.entries(extra)) Object.defineProperty(event, name, { value });
    textarea.dispatchEvent(event);
    return event;
  };
  const settled = () => new Promise((resolve) => setTimeout(resolve, 0));
  return { fire, textarea, pasted, notices, uninstall, settled, textPastes: () => textPastes, focused: () => focused };
}

function serveImages(): string[] {
  const urls: string[] = [];
  globalThis.fetch = async (url) => {
    urls.push(String(url));
    return new Response(JSON.stringify({ path: `/state/images/image-${urls.length}.png` }), { status: 200 });
  };
  return urls;
}

test("a pasted picture is uploaded and its path is pasted; wterm does not see the paste", async () => {
  const urls = serveImages();
  const term = terminal();
  const event = term.fire("paste", "clipboardData", transfer([png()]));
  assert.equal(event.defaultPrevented, true);
  await term.settled();
  assert.deepEqual(urls, ["/api/images"]);
  assert.deepEqual(term.pasted, ["/state/images/image-1.png "]);
  assert.deepEqual(term.notices, ["sending", null]);
  assert.equal(term.textPastes(), 0);
});

test("a text paste stays with wterm", async () => {
  const urls = serveImages();
  const term = terminal();
  const event = term.fire("paste", "clipboardData", transfer([], "ls -la"));
  await term.settled();
  assert.equal(event.defaultPrevented, false);
  assert.equal(term.textPastes(), 1);
  assert.deepEqual([urls, term.pasted, term.notices], [[], [], []]);
});

test("dropped pictures are uploaded in order and pasted as one line", async () => {
  serveImages();
  const term = terminal();
  const over = term.fire("dragover", "dataTransfer", transfer([png()]));
  assert.equal(over.defaultPrevented, true, "the browser must not open the file");
  assert.deepEqual(term.notices, ["drop"]);
  const drop = term.fire("drop", "dataTransfer", transfer([png("a.png"), png("b.png")]));
  assert.equal(drop.defaultPrevented, true);
  await term.settled();
  assert.deepEqual(term.pasted, ["/state/images/image-1.png /state/images/image-2.png "]);
  assert.deepEqual(term.notices, ["drop", "sending", null]);
  assert.equal(term.focused(), 1, "typing continues in the pane that got the drop");
});

test("the drop notice goes when the drag leaves the terminal, not when it moves inside it", () => {
  const term = terminal();
  term.fire("dragover", "dataTransfer", transfer([png()]));
  term.fire("dragleave", "dataTransfer", transfer([png()]), { relatedTarget: term.textarea });
  assert.deepEqual(term.notices, ["drop"]);
  term.fire("dragleave", "dataTransfer", transfer([png()]), { relatedTarget: null, clientX: -1, clientY: -1 });
  assert.deepEqual(term.notices, ["drop", null]);
});

test("a file dropped beside a terminal does not replace the dashboard", () => {
  const browser = new Window({ url: "http://127.0.0.1/" });
  guardFileDrops(browser as unknown as Parameters<typeof guardFileDrops>[0]);
  const fire = (type: string, data: DataTransfer, taken = false) => {
    const event = new browser.Event(type, { bubbles: true, cancelable: true });
    Object.defineProperty(event, "dataTransfer", { value: data });
    if (taken) event.preventDefault();
    browser.document.body.dispatchEvent(event);
    return event;
  };
  const files = transfer([png()]);
  assert.equal(fire("dragover", files).defaultPrevented, true);
  assert.equal(fire("drop", files).defaultPrevented, true);
  assert.equal(files.dropEffect, "none");
  // A terminal took the drag: its drop effect stays.
  const taken = transfer([png()]);
  taken.dropEffect = "copy";
  fire("dragover", taken, true);
  assert.equal(taken.dropEffect, "copy");
  // A pane or tab on the move is not a file.
  assert.equal(fire("dragover", transfer([])).defaultPrevented, false);
});

test("a drag that carries no files is left to the pane's own drag and drop", () => {
  const term = terminal();
  const over = term.fire("dragover", "dataTransfer", transfer([]));
  const drop = term.fire("drop", "dataTransfer", transfer([]));
  assert.equal(over.defaultPrevented || drop.defaultPrevented, false);
  assert.deepEqual(term.notices, []);
});

test("a dropped file that is not a picture is refused with a notice", async () => {
  const urls = serveImages();
  const term = terminal();
  const drop = term.fire("drop", "dataTransfer", transfer([new File(["x"], "notes.txt", { type: "text/plain" })]));
  await term.settled();
  assert.equal(drop.defaultPrevented, true);
  assert.deepEqual([urls, term.pasted, term.notices], [[], [], ["notImage"]]);
});

test("a failed upload pastes nothing and says why", async () => {
  globalThis.fetch = async () => new Response("", { status: 500 });
  const term = terminal();
  term.fire("paste", "clipboardData", transfer([png()]));
  await term.settled();
  assert.deepEqual([term.pasted, term.notices], [[], ["sending", "failed"]]);
});

test("a pane whose program cannot read the server's files takes no pictures", async () => {
  const urls = serveImages();
  const term = terminal(false);
  const paste = term.fire("paste", "clipboardData", transfer([png()]));
  const drop = term.fire("drop", "dataTransfer", transfer([png()]));
  await term.settled();
  assert.equal(paste.defaultPrevented, true);
  assert.equal(drop.defaultPrevented, true, "the browser must not open the file");
  assert.deepEqual([urls, term.pasted, term.notices], [[], [], ["unsupported", "unsupported"]]);
});

test("after uninstall the terminal element has no picture handlers", async () => {
  const urls = serveImages();
  const term = terminal();
  term.uninstall();
  const paste = term.fire("paste", "clipboardData", transfer([png()]));
  await term.settled();
  assert.equal(paste.defaultPrevented, false);
  assert.deepEqual([urls, term.pasted], [[], []]);
});

test("a picture that arrives after the pane closed is not pasted", async () => {
  serveImages();
  const term = terminal();
  term.fire("paste", "clipboardData", transfer([png()]));
  term.uninstall();
  await term.settled();
  assert.deepEqual(term.pasted, []);
});
