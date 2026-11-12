// happy-dom as the globals. Imported before react-dom loads (dom-page.ts
// does it first): react-dom looks at the DOM once, when it loads, to decide
// which events it listens to.

import { Window } from "happy-dom";

// I18nProvider follows the browser's language, which Node takes from LANG.
Object.defineProperty(navigator, "languages", { value: ["en-US"], configurable: true });

const page = new Window({ url: "http://westerm.test/" });
const globals = globalThis as Record<string, unknown>;
for (const name of [
  "window",
  "document",
  "HTMLElement",
  "HTMLInputElement",
  "HTMLTextAreaElement",
  "Node",
  "MutationObserver",
  "getComputedStyle",
  "localStorage",
  "KeyboardEvent",
  "MouseEvent",
  "Event",
]) {
  Object.defineProperty(globals, name, { value: (page as unknown as Record<string, unknown>)[name], configurable: true, writable: true });
}
globals.IS_REACT_ACT_ENVIRONMENT = true;

