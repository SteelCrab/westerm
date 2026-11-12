import type { WTerm } from "@wterm/dom";
import { REMOTE_TARGET } from "../remoteWindow.ts";

/**
 * Pictures pasted or dropped on a terminal pane, for agent CLIs such as
 * Claude Code and Codex.
 *
 * A native terminal pastes the path of a dropped file, and the agent reads
 * the file. A page has the picture's bytes but no path, and the shell may be
 * on another computer (a remote window, a tunnel). So the page sends the
 * picture to the server that runs the shell (native/src/server/images.rs),
 * and pastes the path the server saved it at.
 *
 * @wterm/dom reads only text from a paste and has no drop handler.
 */

/** Keep in step with MAX_IMAGE_BYTES in native/src/server/images.rs. */
export const MAX_IMAGE_BYTES = 10 * 1024 * 1024;

/** The formats the server saves; it checks the bytes, not this name. */
const IMAGE_TYPES: readonly string[] = ["image/png", "image/jpeg", "image/gif", "image/webp"];

/**
 * What the pane shows over the terminal. `drop`: files are dragged over it.
 * `unsupported`: this pane's program cannot read the server's files.
 */
export type ImageNotice = "drop" | "sending" | "tooLarge" | "notImage" | "failed" | "unsupported";

export type UploadResult = { ok: true; path: string } | { ok: false; reason: "tooLarge" | "notImage" | "failed" };

/** Where this window sends pictures: its own dashboard, or a remote window's server. */
export function imagesPath(target: string | null = REMOTE_TARGET): string {
  const prefix = target === null ? "" : `/remote/${encodeURIComponent(target)}`;
  return `${prefix}/api/images`;
}

/**
 * A path as one shell word, the way a native terminal pastes a dropped file:
 * backslashes before special characters, or double quotes for a Windows path.
 */
export function shellPath(path: string): string {
  if (/^[A-Za-z]:[\\/]|^\\\\/.test(path)) return /[^A-Za-z0-9_.:\\/-]/.test(path) ? `"${path}"` : path;
  return path.replace(/[^A-Za-z0-9_@%+=:,./-]/g, "\\$&");
}

function isImage(file: File): boolean {
  return IMAGE_TYPES.includes(file.type);
}

/** The files of a paste or drop. Some browsers fill only the items list. */
function carriedFiles(data: DataTransfer): File[] {
  const files = Array.from(data.files ?? []);
  if (files.length > 0) return files;
  return Array.from(data.items ?? [])
    .filter((item) => item.kind === "file")
    .map((item) => item.getAsFile())
    .filter((file): file is File => file !== null);
}

/**
 * The pictures a paste stands for, or none when the paste is text. Text
 * wins: a copied table cell carries its text and a picture of it. A file
 * copied in the file manager carries its name as text, so that is a picture.
 */
export function pastedImages(data: DataTransfer | null): File[] {
  if (!data) return [];
  const images = carriedFiles(data).filter(isImage);
  if (images.length === 0) return [];
  const text = data.getData("text");
  if (!text) return images;
  const names = carriedFiles(data).map((file) => file.name);
  const onlyNames = text.split(/\r?\n/).every((line) => line.trim() === "" || names.includes(line.trim()));
  return onlyNames ? images : [];
}

function base64(bytes: Uint8Array): string {
  let binary = "";
  // In pieces: one call with every byte as an argument overflows the stack.
  for (let start = 0; start < bytes.length; start += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(start, start + 0x8000));
  }
  return btoa(binary);
}

/** Saves one picture on the server; the answer is the file's path there. */
export async function uploadImage(file: File, target: string | null = REMOTE_TARGET): Promise<UploadResult> {
  if (file.size > MAX_IMAGE_BYTES) return { ok: false, reason: "tooLarge" };
  try {
    const data = base64(new Uint8Array(await file.arrayBuffer()));
    const response = await fetch(imagesPath(target), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ data }),
    });
    if (response.status === 413) return { ok: false, reason: "tooLarge" };
    if (response.status === 415) return { ok: false, reason: "notImage" };
    if (!response.ok) return { ok: false, reason: "failed" };
    const body: unknown = await response.json().catch(() => null);
    const path = (body as { path?: unknown } | null)?.path;
    return typeof path === "string" && path.length > 0 ? { ok: true, path } : { ok: false, reason: "failed" };
  } catch {
    return { ok: false, reason: "failed" };
  }
}

/**
 * A file dropped beside a terminal (a pane header, the sidebar) must not make
 * the browser open it in place of the dashboard. Terminals take their own
 * drops first (installImageInput).
 */
export function guardFileDrops(page: { addEventListener(type: "dragover" | "drop", listener: (event: DragEvent) => void): void }): void {
  const refuse = (event: DragEvent) => {
    if (event.defaultPrevented || !carriesFiles(event.dataTransfer)) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = "none";
  };
  page.addEventListener("dragover", refuse);
  page.addEventListener("drop", refuse);
}

export interface ImageInputOptions {
  /** False for a pane whose program cannot read the server's files (ssh, pixel apps). */
  enabled: boolean;
  /** Sends text the way a clipboard paste would. */
  paste: (text: string) => void;
  /** The notice to show over the terminal, or null to clear it. */
  notify: (notice: ImageNotice | null) => void;
}

function carriesFiles(data: DataTransfer | null): data is DataTransfer {
  return data !== null && Array.from(data.types ?? []).includes("Files");
}

export function installImageInput(wt: WTerm, options: ImageInputOptions): () => void {
  const element = wt.element;
  let installed = true;

  const send = async (images: File[]) => {
    options.notify("sending");
    const paths: string[] = [];
    for (const image of images) {
      const result = await uploadImage(image);
      if (!installed) return;
      if (!result.ok) {
        options.notify(result.reason);
        return;
      }
      paths.push(result.path);
    }
    options.notify(null);
    // The space lets the next path, or the prompt's text, follow.
    options.paste(`${paths.map(shellPath).join(" ")} `);
  };

  const onPaste = (event: ClipboardEvent) => {
    const images = pastedImages(event.clipboardData);
    if (images.length === 0) return; // Text: wterm's own listener sends it.
    event.preventDefault();
    event.stopPropagation();
    if (options.enabled) void send(images);
    else options.notify("unsupported");
  };

  const onDragOver = (event: DragEvent) => {
    if (!carriesFiles(event.dataTransfer)) return; // A pane or tab on the move.
    // Without this the browser opens the file in place of the dashboard.
    event.preventDefault();
    event.dataTransfer.dropEffect = options.enabled ? "copy" : "none";
    if (options.enabled) options.notify("drop");
  };

  const onDragLeave = (event: DragEvent) => {
    if (!options.enabled || !carriesFiles(event.dataTransfer)) return;
    // The drag also "leaves" each child it moves off. Safari names no next
    // element, so there the pointer's place decides.
    const next = event.relatedTarget as Node | null;
    const rect = element.getBoundingClientRect();
    const inside = next
      ? element.contains(next)
      : event.clientX > rect.left && event.clientX < rect.right && event.clientY > rect.top && event.clientY < rect.bottom;
    if (!inside) options.notify(null);
  };

  const onDrop = (event: DragEvent) => {
    if (!carriesFiles(event.dataTransfer)) return;
    event.preventDefault();
    if (!options.enabled) {
      options.notify("unsupported");
      return;
    }
    const images = carriedFiles(event.dataTransfer).filter(isImage);
    if (images.length === 0) {
      options.notify("notImage");
      return;
    }
    wt.focus();
    void send(images);
  };

  // Capture on the terminal element runs before wterm's textarea listener.
  element.addEventListener("paste", onPaste, true);
  element.addEventListener("dragover", onDragOver);
  element.addEventListener("dragleave", onDragLeave);
  element.addEventListener("drop", onDrop);
  return () => {
    installed = false;
    element.removeEventListener("paste", onPaste, true);
    element.removeEventListener("dragover", onDragOver);
    element.removeEventListener("dragleave", onDragLeave);
    element.removeEventListener("drop", onDrop);
  };
}
