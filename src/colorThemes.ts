import type { TerminalThemeColors } from "@wterm/core";
import type { ColorScheme } from "./theme.ts";

/**
 * Color themes (Settings ▸ Appearance ▸ Color theme). Each has a dark and a light set of
 * terminal colors that follow Settings ▸ Appearance ▸ Mode, and the window takes its
 * colors from them (uiColors). WTerm.setThemeColors() hands the terminal
 * colors to the Ghostty core too, so programs that ask for the background
 * (OSC 11), such as terminal-browser and Neovim, pick a matching theme.
 */
export const COLOR_THEMES = [
  "default",
  "one",
  "github",
  "solarized",
  "gruvbox",
  "catppuccin",
  "tokyo-night",
  "rose-pine",
  "everforest",
  "kanagawa",
] as const;
export type ColorThemeId = (typeof COLOR_THEMES)[number];

export function isColorTheme(value: unknown): value is ColorThemeId {
  return (COLOR_THEMES as readonly unknown[]).includes(value);
}

interface TerminalTheme {
  /** The theme's own name; "default" takes a translated name instead. */
  name: string;
  dark: TerminalThemeColors;
  light: TerminalThemeColors;
}

/** One Light: the light set of both the default theme and One. */
const ONE_LIGHT: TerminalThemeColors = {
  foreground: 0x383a42,
  background: 0xfafafa,
  cursor: 0x526fff,
  palette: [
    0x383a42, 0xe45649, 0x50a14f, 0xc18401, 0x4078f2, 0xa626a4, 0x0184bc, 0xa0a1a7,
    0x4f525e, 0xe45649, 0x50a14f, 0xc18401, 0x4078f2, 0xa626a4, 0x0184bc, 0xffffff,
  ],
};

const THEMES: Record<ColorThemeId, TerminalTheme> = {
  default: {
    name: "Default",
    dark: {
      foreground: 0xd4d4d4,
      background: 0x1e1e1e,
      cursor: 0xaeafad,
      palette: [
        0x1e1e1e, 0xf44747, 0x6a9955, 0xd7ba7d, 0x569cd6, 0xc586c0, 0x4ec9b0, 0xd4d4d4,
        0x808080, 0xf44747, 0x6a9955, 0xd7ba7d, 0x569cd6, 0xc586c0, 0x4ec9b0, 0xffffff,
      ],
    },
    light: ONE_LIGHT,
  },
  one: {
    name: "One",
    dark: {
      foreground: 0xabb2bf,
      background: 0x282c34,
      cursor: 0x528bff,
      palette: [
        0x282c34, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xabb2bf,
        0x5c6370, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xffffff,
      ],
    },
    light: ONE_LIGHT,
  },
  github: {
    name: "GitHub",
    dark: {
      foreground: 0xe6edf3,
      background: 0x0d1117,
      cursor: 0x2f81f7,
      palette: [
        0x484f58, 0xff7b72, 0x3fb950, 0xd29922, 0x58a6ff, 0xbc8cff, 0x39c5cf, 0xb1bac4,
        0x6e7681, 0xffa198, 0x56d364, 0xe3b341, 0x79c0ff, 0xd2a8ff, 0x56d4dd, 0xffffff,
      ],
    },
    light: {
      foreground: 0x1f2328,
      background: 0xffffff,
      cursor: 0x0969da,
      palette: [
        0x24292f, 0xcf222e, 0x116329, 0x4d2d00, 0x0969da, 0x8250df, 0x1b7c83, 0x6e7781,
        0x57606a, 0xa40e26, 0x1a7f37, 0x633c01, 0x218bff, 0xa475f9, 0x3192aa, 0x8c959f,
      ],
    },
  },
  solarized: {
    name: "Solarized",
    dark: {
      foreground: 0x839496,
      background: 0x002b36,
      cursor: 0x93a1a1,
      palette: [
        0x073642, 0xdc322f, 0x859900, 0xb58900, 0x268bd2, 0xd33682, 0x2aa198, 0xeee8d5,
        0x586e75, 0xcb4b16, 0x859900, 0xb58900, 0x268bd2, 0x6c71c4, 0x2aa198, 0xfdf6e3,
      ],
    },
    light: {
      // base01, one step darker than the usual base00, keeps text at 4.5:1.
      foreground: 0x586e75,
      background: 0xfdf6e3,
      cursor: 0x586e75,
      palette: [
        0x073642, 0xdc322f, 0x859900, 0xb58900, 0x268bd2, 0xd33682, 0x2aa198, 0xeee8d5,
        0x657b83, 0xcb4b16, 0x859900, 0xb58900, 0x268bd2, 0x6c71c4, 0x2aa198, 0xfdf6e3,
      ],
    },
  },
  gruvbox: {
    name: "Gruvbox",
    dark: {
      foreground: 0xebdbb2,
      background: 0x282828,
      cursor: 0xebdbb2,
      palette: [
        0x282828, 0xcc241d, 0x98971a, 0xd79921, 0x458588, 0xb16286, 0x689d6a, 0xa89984,
        0x928374, 0xfb4934, 0xb8bb26, 0xfabd2f, 0x83a598, 0xd3869b, 0x8ec07c, 0xebdbb2,
      ],
    },
    light: {
      foreground: 0x3c3836,
      background: 0xfbf1c7,
      cursor: 0x3c3836,
      palette: [
        0xfbf1c7, 0xcc241d, 0x98971a, 0xd79921, 0x458588, 0xb16286, 0x689d6a, 0x7c6f64,
        0x928374, 0x9d0006, 0x79740e, 0xb57614, 0x076678, 0x8f3f71, 0x427b58, 0x3c3836,
      ],
    },
  },
  catppuccin: {
    name: "Catppuccin",
    // Mocha
    dark: {
      foreground: 0xcdd6f4,
      background: 0x1e1e2e,
      cursor: 0xf5e0dc,
      palette: [
        0x45475a, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xbac2de,
        0x585b70, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xa6adc8,
      ],
    },
    // Latte
    light: {
      foreground: 0x4c4f69,
      background: 0xeff1f5,
      cursor: 0xdc8a78,
      palette: [
        0x5c5f77, 0xd20f39, 0x40a02b, 0xdf8e1d, 0x1e66f5, 0xea76cb, 0x179299, 0xacb0be,
        0x6c6f85, 0xd20f39, 0x40a02b, 0xdf8e1d, 0x1e66f5, 0xea76cb, 0x179299, 0xbcc0cc,
      ],
    },
  },
  "tokyo-night": {
    name: "Tokyo Night",
    // Night
    dark: {
      foreground: 0xc0caf5,
      background: 0x1a1b26,
      cursor: 0xc0caf5,
      palette: [
        0x15161e, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xa9b1d6,
        0x414868, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xc0caf5,
      ],
    },
    // Day
    light: {
      foreground: 0x3760bf,
      background: 0xe1e2e7,
      cursor: 0x3760bf,
      palette: [
        0xe9e9ed, 0xf52a65, 0x587539, 0x8c6c3e, 0x2e7de9, 0x9854f1, 0x007197, 0x6172b0,
        0xa1a6c5, 0xf52a65, 0x587539, 0x8c6c3e, 0x2e7de9, 0x9854f1, 0x007197, 0x3760bf,
      ],
    },
  },
  "rose-pine": {
    name: "Rosé Pine",
    // Main
    dark: {
      foreground: 0xe0def4,
      background: 0x191724,
      cursor: 0xebbcba,
      palette: [
        0x26233a, 0xeb6f92, 0x31748f, 0xf6c177, 0x9ccfd8, 0xc4a7e7, 0xebbcba, 0xe0def4,
        0x6e6a86, 0xeb6f92, 0x31748f, 0xf6c177, 0x9ccfd8, 0xc4a7e7, 0xebbcba, 0xe0def4,
      ],
    },
    // Dawn
    light: {
      foreground: 0x575279,
      background: 0xfaf4ed,
      cursor: 0xd7827e,
      palette: [
        0xf2e9e1, 0xb4637a, 0x286983, 0xea9d34, 0x56949f, 0x907aa9, 0xd7827e, 0x575279,
        0x9893a5, 0xb4637a, 0x286983, 0xea9d34, 0x56949f, 0x907aa9, 0xd7827e, 0x575279,
      ],
    },
  },
  everforest: {
    name: "Everforest",
    // Medium contrast
    dark: {
      foreground: 0xd3c6aa,
      background: 0x2d353b,
      cursor: 0xd3c6aa,
      palette: [
        0x475258, 0xe67e80, 0xa7c080, 0xdbbc7f, 0x7fbbb3, 0xd699b6, 0x83c092, 0xd3c6aa,
        0x7a8478, 0xe67e80, 0xa7c080, 0xdbbc7f, 0x7fbbb3, 0xd699b6, 0x83c092, 0xd3c6aa,
      ],
    },
    light: {
      foreground: 0x5c6a72,
      background: 0xfdf6e3,
      cursor: 0x5c6a72,
      palette: [
        0x5c6a72, 0xf85552, 0x8da101, 0xdfa000, 0x3a94c5, 0xdf69ba, 0x35a77c, 0xe6e2cc,
        0x939f91, 0xf85552, 0x8da101, 0xdfa000, 0x3a94c5, 0xdf69ba, 0x35a77c, 0xfdf6e3,
      ],
    },
  },
  kanagawa: {
    name: "Kanagawa",
    // Wave
    dark: {
      foreground: 0xdcd7ba,
      background: 0x1f1f28,
      cursor: 0xc8c093,
      palette: [
        0x090618, 0xc34043, 0x76946a, 0xc0a36e, 0x7e9cd8, 0x957fb8, 0x6a9589, 0xc8c093,
        0x727169, 0xe82424, 0x98bb6c, 0xe6c384, 0x7fb4ca, 0x938aa9, 0x7aa89f, 0xdcd7ba,
      ],
    },
    // Lotus
    light: {
      foreground: 0x545464,
      background: 0xf2ecbc,
      cursor: 0x43436c,
      palette: [
        0x1f1f28, 0xc84053, 0x6f894e, 0x77713f, 0x4d699b, 0xb35b79, 0x597b75, 0x545464,
        0x8a8980, 0xd7474b, 0x6e915f, 0x836f4a, 0x6693bf, 0x624c83, 0x5e857a, 0x43436c,
      ],
    },
  },
};

/** The theme's colors for the scheme in use. */
export function terminalColors(id: ColorThemeId, scheme: ColorScheme): TerminalThemeColors {
  return THEMES[id][scheme];
}

/** The theme's own name; null for the default theme, whose name is translated. */
export function themeName(id: ColorThemeId): string | null {
  return id === "default" ? null : THEMES[id].name;
}

/** 0xRRGGBB as a CSS color. */
export function cssColor(value: number): string {
  return `#${value.toString(16).padStart(6, "0")}`;
}

/** 0xRRGGBB moved toward `to` by `amount` (0 to 1), channel by channel. */
function mix(from: number, to: number, amount: number): number {
  let out = 0;
  for (const shift of [16, 8, 0]) {
    const a = (from >> shift) & 0xff;
    const b = (to >> shift) & 0xff;
    out |= Math.round(a + (b - a) * amount) << shift;
  }
  return out;
}

function luminance(color: number): number {
  const [r, g, b] = [16, 8, 0].map((shift) => {
    const c = ((color >> shift) & 0xff) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** WCAG contrast ratio. */
function contrast(a: number, b: number): number {
  const [light, dark] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (light + 0.05) / (dark + 0.05);
}

const readable = (color: number, grounds: number[], ratio: number) =>
  grounds.every((ground) => contrast(color, ground) >= ratio);

/** `color`, moved toward the text color just enough to reach `ratio` on every ground. */
function reach(color: number, grounds: number[], ratio: number, text: number): number {
  for (let step = 0; step <= 20; step++) {
    const candidate = mix(color, text, step / 20);
    if (readable(candidate, grounds, ratio)) return candidate;
  }
  return text;
}

/** The text color, faded toward the surface as far as `ratio` allows. */
function fade(text: number, grounds: number[], ratio: number, most: number): number {
  for (let step = Math.round(most * 20); step > 0; step--) {
    const candidate = mix(text, grounds[0], step / 20);
    if (readable(candidate, grounds, ratio)) return candidate;
  }
  return text;
}

const rgba = (color: number, alpha: number) =>
  `rgba(${(color >> 16) & 0xff}, ${(color >> 8) & 0xff}, ${color & 0xff}, ${alpha})`;

/**
 * The window's colors for a theme, as CSS custom properties that override
 * the stylesheet's; null for the default theme, which keeps the stylesheet.
 * They come from the terminal colors: the surfaces from the background, the
 * text from the foreground, the accent from blue, and the status and kind
 * colors from the palette. Text keeps 4.5:1 on every surface, and marks
 * and icons 3:1 (tests/color-themes.test.ts).
 */
export function uiColors(id: ColorThemeId, scheme: ColorScheme): Record<string, string> | null {
  if (id === "default") return null;
  const { foreground: text, background: pane, palette } = terminalColors(id, scheme);
  const dark = scheme === "dark";
  // Dark: the frame is darker than the terminal. Light: the surfaces are lighter, the frame a bit darker.
  const surface = dark ? mix(pane, 0x000000, 0.18) : mix(pane, 0xffffff, 0.4);
  const bg = dark ? mix(pane, 0x000000, 0.35) : mix(surface, text, 0.05);
  const surface2 = dark ? mix(pane, 0x000000, 0.06) : mix(surface, text, 0.07);
  const grounds = [surface, bg, surface2];
  const accent = reach(palette[4], grounds, 3, text);
  const browser = reach(palette[6], grounds, 3, text);
  const editor = reach(palette[5], grounds, 3, text);
  const ssh = reach(mix(palette[1], palette[3], 0.5), grounds, 3, text);
  const soft = dark ? 0.16 : 0.12;
  return {
    "--bg": cssColor(bg),
    "--surface": cssColor(surface),
    "--surface-2": cssColor(surface2),
    "--border": cssColor(mix(surface, text, dark ? 0.12 : 0.14)),
    "--border-strong": cssColor(mix(surface, text, dark ? 0.2 : 0.22)),
    "--text": cssColor(text),
    "--text-muted": cssColor(fade(text, grounds, 4.5, 0.45)),
    "--text-faint": cssColor(fade(text, grounds, 3, 0.65)),
    "--accent": cssColor(accent),
    "--accent-soft": rgba(accent, soft),
    "--on-accent": contrast(accent, 0x000000) >= contrast(accent, 0xffffff) ? "#000000" : "#ffffff",
    "--browser": cssColor(browser),
    "--browser-soft": rgba(browser, soft),
    "--editor": cssColor(editor),
    "--editor-soft": rgba(editor, soft),
    "--ssh": cssColor(ssh),
    "--ssh-soft": rgba(ssh, soft),
    "--ok": cssColor(reach(palette[2], grounds, 3, text)),
    "--warn": cssColor(reach(palette[3], grounds, 4.5, text)),
    "--danger": cssColor(reach(palette[1], grounds, 4.5, text)),
    "--pane-bg": cssColor(pane),
    "--overlay": rgba(bg, dark ? 0.78 : 0.82),
  };
}
