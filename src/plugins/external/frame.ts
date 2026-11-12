// An external plugin's iframe. A browser reloads an iframe that moves in the
// DOM, so it is inserted once, into the frame layer under <body>, and never
// moved: `place` lays it over the panel's slot with `position: fixed`
// instead, and hides it when there is no slot.

export interface Frame {
  element: HTMLIFrameElement;
  /** Lays the frame over `slot` and follows its size; null hides it. */
  place(slot: HTMLElement | null): void;
  remove(): void;
}

export interface FrameOptions {
  src: string;
  title: string;
  /** The frame layer; the iframe stays its child until removed. */
  host: HTMLElement;
  onFirstLoad(): void;
  /** A load after the first: the page navigated away from itself. */
  onNavigate(): void;
}

export function createFrame(o: FrameOptions): Frame {
  const element = document.createElement("iframe");
  // Scripts only: an opaque origin, no popups, forms, top navigation, or modals.
  element.setAttribute("sandbox", "allow-scripts");
  element.setAttribute("referrerpolicy", "no-referrer");
  element.title = o.title;
  element.className = "plugin-frame";
  element.hidden = true;

  let loads = 0;
  const onLoad = () => {
    loads += 1;
    if (loads === 1) o.onFirstLoad();
    else o.onNavigate();
  };
  element.addEventListener("load", onLoad);
  element.src = o.src;
  o.host.appendChild(element);

  let slot: HTMLElement | null = null;
  let observer: ResizeObserver | null = null;

  const fit = () => {
    if (slot === null) return;
    const box = slot.getBoundingClientRect();
    element.style.left = `${box.left}px`;
    element.style.top = `${box.top}px`;
    element.style.width = `${box.width}px`;
    element.style.height = `${box.height}px`;
  };

  const release = () => {
    observer?.disconnect();
    observer = null;
    window.removeEventListener("resize", fit);
    slot = null;
  };

  return {
    element,
    place(next) {
      if (next === slot) {
        fit();
        return;
      }
      release();
      element.hidden = next === null;
      if (next === null) return;
      slot = next;
      if (typeof ResizeObserver === "function") {
        observer = new ResizeObserver(fit);
        observer.observe(next);
      }
      window.addEventListener("resize", fit);
      fit();
    },
    remove() {
      release();
      element.removeEventListener("load", onLoad);
      element.remove();
    },
  };
}
