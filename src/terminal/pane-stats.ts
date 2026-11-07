/**
 * Counters for pixel app panes, read by bench/pixel as
 * `globalThis.__westermStats`. They only add numbers, so they stay on.
 *
 * frames: pictures that reached the pane. With pixel updates, updates drawn;
 * with Kitty images, synchronized updates the terminal core parsed.
 */
export interface PaneStats {
  frames: number;
  textBytes: number;
  pixelBytes: number;
  pixelUpdates: number;
}

export const paneStats: PaneStats = { frames: 0, textBytes: 0, pixelBytes: 0, pixelUpdates: 0 };

(globalThis as { __westermStats?: PaneStats }).__westermStats = paneStats;
