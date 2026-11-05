/** The parts of the page that decide who has the keyboard. */
interface Page {
  querySelector(selector: string): unknown;
  activeElement: { classList?: { contains(name: string): boolean } } | null;
}

/**
 * Whether a terminal may take the keyboard: not while a dialog is open, and
 * not from a rename field. A palette command that starts a rename also closes
 * the palette, and closing it asks the terminal to take the keyboard back.
 */
export function keyboardFree(page: Page = document): boolean {
  if (page.querySelector('[aria-modal="true"]') !== null) return false;
  return !page.activeElement?.classList?.contains("inline-rename");
}
