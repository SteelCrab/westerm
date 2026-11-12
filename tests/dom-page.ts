// happy-dom as the globals, for tests that mount React and click. Import it
// first; the file is not a test itself (tests/*.test.ts).

// The globals come first: react-dom checks for DOM events when it loads.
import "./dom-globals.ts";
import assert from "node:assert/strict";
import { act, createElement, type ReactElement } from "react";
import { createRoot } from "react-dom/client";
import { I18nProvider } from "../src/i18n/index.tsx";

export const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

export interface Mounted {
  container: HTMLElement;
  rerender(element: ReactElement): Promise<void>;
  unmount(): Promise<void>;
}

export async function mount(element: ReactElement): Promise<Mounted> {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  const render = async (next: ReactElement) => {
    await act(async () => {
      root.render(createElement(I18nProvider, null, next));
      await flush();
    });
  };
  await render(element);
  return {
    container,
    rerender: render,
    async unmount() {
      await act(async () => root.unmount());
      container.remove();
    },
  };
}

/** Clicks `element` and lets the answers in. */
export async function click(element: Element | null | undefined): Promise<void> {
  assert.ok(element, "the control is there");
  await act(async () => {
    (element as HTMLElement).click();
    for (let i = 0; i < 4; i += 1) await flush();
  });
}

/** Lets pending promises and timers of 0 ms run. */
export async function settle(rounds = 4): Promise<void> {
  await act(async () => {
    for (let i = 0; i < rounds; i += 1) await flush();
  });
}

/** Types `value` into an input or textarea the way React notices. */
export async function type(element: Element | null | undefined, value: string): Promise<void> {
  assert.ok(element, "the field is there");
  const field = element as HTMLInputElement;
  const proto = field.tagName === "TEXTAREA" ? window.HTMLTextAreaElement.prototype : window.HTMLInputElement.prototype;
  const setter = Object.getOwnPropertyDescriptor(proto, "value")?.set;
  await act(async () => {
    setter?.call(field, value);
    field.dispatchEvent(new window.Event("input", { bubbles: true }) as unknown as Event);
    await flush();
  });
}

/** The visible text of `element`, spaces collapsed. */
export function text(element: Element): string {
  return (element.textContent ?? "").replace(/\s+/g, " ").trim();
}
