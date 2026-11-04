/**
 * Calls `end` once when a drag that started on a handle finishes anywhere:
 * the button is released (also outside the handle or the window), the drag
 * is cancelled, or the window loses focus. Handles that only listen on
 * themselves can miss the release and stay in "resizing" mode.
 */
export function onDragEnd(end: () => void): void {
  const events = ["pointerup", "pointercancel", "mouseup"] as const;
  const finish = () => {
    for (const name of events) window.removeEventListener(name, finish, true);
    window.removeEventListener("blur", finish);
    end();
  };
  for (const name of events) window.addEventListener(name, finish, true);
  window.addEventListener("blur", finish);
}
