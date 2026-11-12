// Small pure helpers shared by core components and plugins. Imports nothing,
// so a plugin can use them without reaching the core's state.

/** Whole minutes from `time` to `now`, both in milliseconds since the epoch. */
export function minutesSince(time: number, now: number): number {
  return Math.max(0, Math.floor((now - time) / 60_000));
}

/** "~/westerm" for a folder in the user's home, `~\westerm` on Windows; other paths as they are. */
export function homeShort(path: string): string {
  return path.replace(/^\/(?:Users|home)\/[^/]+(?=\/|$)/, "~").replace(/^[A-Za-z]:\\Users\\[^\\]+(?=\\|$)/, "~");
}

/** The menu item that an arrow key moves to; -1 is "none focused yet". */
export function stepIndex(current: number, delta: 1 | -1, count: number): number {
  if (current < 0) return delta > 0 ? 0 : count - 1;
  return (current + delta + count) % count;
}
