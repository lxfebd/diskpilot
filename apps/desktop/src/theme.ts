// DiskPilot theme system — 17 presets drawn from the J:\UI deg reference
// library (each kit's own token ramp: bg/surface/text/brand/border/shadows).
// Everything lives in CSS custom properties on <html>: applyTheme() writes
// inline properties, which win the cascade over the :root defaults in
// styles.css. Settings persists to localStorage under "diskpilot.theme" and
// broadcasts a window event so any open Settings dialog can react.
//
// Every preset defines the full semantic layer (--bg/--surface/--text/
// --accent/--risk-* + raw --ink/--paper/--pink legacy names) so switching is
// instant; the "accent" raw tint is kept for legacy components while the
// semantic --accent-* chain is what new code consumes.

// 17 kits ↔ preset ids:
//   vercel   → Vercel Design System (tech dashboard, 1px quiet borders)
//   minimal  → Minimalist (monochrome, cool-neutral brand scale)
//   doubao   → Doubao (byte-light, readable docs)
//   claude   → Claude (warm cream, editorial serif)
//   google   → Google (analytical clean, blue link)
//   volcengine → Volcengine (yuanli blue, dark tech)
//   nerv     → Nerv (dark neon editorial, high contrast)
//   trae     → TRAE Work (engineering studio, violet)
//   motionfit → Motion Fit (dark energetic, signal orange)
//   barbie   → Barbie (playful bubblegum, big radius)
//   brand    → Apple Copy (restrained elegant, system blue)
//   golden   → Golden Time (warm editorial, earth gold)
//   vibecamp → Vibe Camp (terracotta clay, rustic)
//   21th     → 21st (macOS graphite minimal, square radius)
//   steam    → Steam-ish dark dwarven tech green
//   dark     → Nerv dark (kept for compatibility)
//   pro      → Volcengine dark (kept for compatibility)
export type ThemeId =
  | 'vercel' | 'minimal' | 'doubao' | 'claude' | 'google' | 'volcengine'
  | 'nerv' | 'trae' | 'motionfit' | 'barbie' | 'brand' | 'golden'
  | 'vibecamp' | '21th' | 'steam' | 'dark' | 'pro';

export interface ThemeSettings {
  id: ThemeId;
  /** Custom accent hex (#rrggbb), or null = preset default. */
  accent: string | null;
  /** 0.8 – 1.3, multiplies the 13px base font size. */
  fontScale: number;
}

const KEY = 'diskpilot.theme';

type C = Record<string, string>;

// Font ramps per family — the kits pair their color language with a type
// voice; we tighten weights (no 800/900) to keep the tool quiet.
export const THEME_FONTS: Record<ThemeId, { ui: string; editor: string }> = {
  vercel:     { ui: 'Geist, Inter, -apple-system, "Segoe UI", Roboto, sans-serif', editor: 'ui-monospace, "Cascadia Code", Consolas, monospace' },
  minimal:    { ui: 'Inter, -apple-system, "Segoe UI", Roboto, sans-serif', editor: 'ui-monospace, "Cascadia Code", Consolas, monospace' },
  doubao:     { ui: 'Inter, -apple-system, "PingFang SC", "Microsoft YaHei", sans-serif', editor: 'ui-monospace, "Cascadia Code", Consolas, monospace' },
  claude:     { ui: 'Newsreader, Lora, Georgia, "Songti SC", serif', editor: 'ui-monospace, "Cascadia Code", Consolas, monospace' },
  google:     { ui: 'Roboto, "Google Sans", Inter, -apple-system, sans-serif', editor: 'ui-monospace, "Cascadia Code", Consolas, monospace' },
  volcengine: { ui: 'Inter, "PingFang SC", "Microsoft YaHei", sans-serif', editor: 'ui-monospace, "Cascadia Code", Consolas, monospace' },
  nerv:       { ui: 'Inter, "Segoe UI", -apple-system, sans-serif', editor: 'ui-monospace, "Cascadia Code", Consolas, monospace' },
  trae:       { ui: 'Inter, "PingFang SC", "Microsoft YaHei", sans-serif', editor: 'ui-monospace, "Cascadia Code", Consolas, monospace' },
  motionfit:  { ui: 'Inter, "Segoe UI", sans-serif', editor: 'ui-monospace, "Cascadia Code", Consolas, monospace' },
  barbie:     { ui: '"Comic Sans MS", "Sniglet", "Baloo 2", "PingFang SC", sans-serif', editor: 'ui-monospace, "Cascadia Code", Consolas, monospace' },
  brand:      { ui: '-apple-system, Inter, "PingFang SC", sans-serif', editor: 'ui-monospace, "Cascadia Code", Consolas, monospace' },
  golden:     { ui: 'Lora, Georgia, "Songti SC", serif', editor: 'ui-monospace, "Cascadia Code", Consolas, monospace' },
  vibecamp:   { ui: 'Avenir, "PingFang SC", "Microsoft YaHei", sans-serif', editor: 'ui-monospace, "Cascadia Code", Consolas, monospace' },
  '21th':     { ui: '-apple-system, "SF Pro Text", Inter, sans-serif', editor: 'ui-monospace, "Cascadia Code", Consolas, monospace' },
  steam:      { ui: 'Segoe UI, "PingFang SC", "Microsoft YaHei", sans-serif', editor: 'ui-monospace, "Cascadia Code", Consolas, monospace' },
  dark:       { ui: 'Inter, "Segoe UI", -apple-system, sans-serif', editor: 'ui-monospace, "Cascadia Code", Consolas, monospace' },
  pro:        { ui: 'Inter, "Segoe UI", sans-serif', editor: 'ui-monospace, "Cascadia Code", Consolas, monospace' },
};

const PRESETS: Record<ThemeId, C> = {
  // ── Vercel Design System ───────────────────────────────────────────────
  // #fff/#0a0a0a monochrome, 1px #e8e8e8 borders, near-zero shadows,
  // primary #121212, single blue accent. Navigation = contrast shift not fill.
  vercel: {
    '--ink': '#0a0a0a', '--ink-2': '#3f3f3f', '--ink-3': '#858585',
    '--paper': '#fafafa', '--paper-2': '#f5f5f5', '--paper-3': '#ebebeb', '--card': '#ffffff',
    '--pink': '#121212', '--pink-deep': '#404040', '--pink-soft': '#f5f5f5', '--pink-bg': '#f5f5f5',
    '--lavender': '#8b8b9e', '--mint': '#62d178', '--peach': '#f0a35e', '--sun': '#d4a72c',
    '--ok': '#16a34a', '--warn': '#b45309', '--err': '#e7000b',
    '--line': '#e8e8e8', '--border': '#e8e8e8', '--border-strong': '#d4d4d4',
    '--shadow-sm': '0 1px 2px rgba(0,0,0,.04)',
    '--shadow': '0 1px 2px rgba(0,0,0,.04), 0 2px 8px -2px rgba(0,0,0,.06)',
    '--shadow-lg': '0 4px 12px -2px rgba(0,0,0,.1)',
    '--bg': '#fafafa', '--surface': '#ffffff', '--surface-2': '#f5f5f5', '--surface-3': '#ebebeb',
    '--paper-1': '#f5f5f5', '--muted': '#858585',
    '--text': '#0a0a0a', '--text-2': '#3f3f3f', '--text-3': '#858585',
    '--accent': '#1447e6', '--accent-strong': '#0f35ad', '--accent-soft': '#e8efff', '--accent-bg': '#f0f4ff',
    '--risk-safe': '#16a34a', '--risk-caution': '#b45309', '--risk-danger': '#e7000b',
  },
  // ── Minimalist ─────────────────────────────────────────────────────────
  // Monochrome: cool-neutral brand scale (50 #fafafa → 900 #18181b), pure
  // grays; primary=brand-900, secondary=brand-100, ring=brand-900, radius .75rem.
  minimal: {
    '--ink': '#18181b', '--ink-2': '#52525b', '--ink-3': '#71717a',
    '--paper': '#fafafa', '--paper-2': '#f4f4f5', '--paper-3': '#e4e4e7', '--card': '#ffffff',
    '--pink': '#18181b', '--pink-deep': '#3f3f46', '--pink-soft': '#f4f4f5', '--pink-bg': '#f4f4f5',
    '--lavender': '#8f8fa3', '--mint': '#6bbf8e', '--peach': '#d9a37a', '--sun': '#c9c06d',
    '--ok': '#2f9e63', '--warn': '#b07a2e', '--err': '#dc2626',
    '--line': '#e4e4e7', '--border': '#e4e4e7', '--border-strong': '#d4d4d8',
    '--shadow-sm': '0 1px 2px rgba(24,24,27,.05)',
    '--shadow': '0 2px 6px rgba(24,24,27,.07)',
    '--shadow-lg': '0 8px 24px rgba(24,24,27,.1)',
    '--bg': '#fafafa', '--surface': '#ffffff', '--surface-2': '#f4f4f5', '--surface-3': '#e4e4e7',
    '--paper-1': '#f4f4f5', '--muted': '#71717a',
    '--text': '#18181b', '--text-2': '#3f3f46', '--text-3': '#71717a',
    '--accent': '#27272a', '--accent-strong': '#18181b', '--accent-soft': '#f4f4f5', '--accent-bg': '#f4f4f5',
    '--risk-safe': '#2f9e63', '--risk-caution': '#b07a2e', '--risk-danger': '#dc2626',
    '--radius': '0.75rem',
  },
  // ── Doubao ─────────────────────────────────────────────────────────────
  // #fff/#0e1115, card #fff, muted #eff1f4, border #e7eaef, primary #0065fd
  // (byte-blue), accent #e5e9ff, radius 1.2rem. Light, readable docs style.
  doubao: {
    '--ink': '#0e1115', '--ink-2': '#333942', '--ink-3': '#7f8d9f',
    '--paper': '#f9f9fa', '--paper-2': '#eff1f4', '--paper-3': '#e7eaef', '--card': '#ffffff',
    '--pink': '#0065fd', '--pink-deep': '#004bb8', '--pink-soft': '#e5e9ff', '--pink-bg': '#eff4ff',
    '--lavender': '#8f92e0', '--mint': '#35d0a0', '--peach': '#ff9d68', '--sun': '#f7c948',
    '--ok': '#00b578', '--warn': '#d9800a', '--err': '#ef4444',
    '--line': '#e7eaef', '--border': '#e7eaef', '--border-strong': '#d0d6de',
    '--shadow-sm': '0 1px 2px rgba(14,17,21,.05)',
    '--shadow': '0 2px 8px rgba(14,17,21,.07)',
    '--shadow-lg': '0 8px 24px rgba(14,17,21,.1)',
    '--bg': '#f9f9fa', '--surface': '#ffffff', '--surface-2': '#eff1f4', '--surface-3': '#e7eaef',
    '--paper-1': '#eff1f4', '--muted': '#7f8d9f',
    '--text': '#0e1115', '--text-2': '#333942', '--text-3': '#7f8d9f',
    '--accent': '#0065fd', '--accent-strong': '#004bb8', '--accent-soft': '#d9e8ff', '--accent-bg': '#eff4ff',
    '--risk-safe': '#00b578', '--risk-caution': '#d9800a', '--risk-danger': '#ef4444',
    '--radius': '1.2rem',
  },
  // ── Claude (Anthropic) ─────────────────────────────────────────────────
  // Warm cream surfaces (bg-100 #faf9f5, card bg-200 #f5f4ef), text-800
  // olive-black, brand-500 #c96442 terracotta, secondary #e9e6dc sand,
  // border-300 #e6e3d8, radius 1rem. Editorial serif Newsreader/Lora.
  claude: {
    '--ink': '#3d3929', '--ink-2': '#535146', '--ink-3': '#6e6d68',
    '--paper': '#faf9f5', '--paper-2': '#f5f4ef', '--paper-3': '#ede9de', '--card': '#ffffff',
    '--pink': '#c96442', '--pink-deep': '#934828', '--pink-soft': '#f4e0d5', '--pink-bg': '#fbf2ed',
    '--lavender': '#8f8fa0', '--mint': '#7aa887', '--peach': '#d99b6a', '--sun': '#c9b25d',
    '--ok': '#3e7a52', '--warn': '#a56a2e', '--err': '#b0503a',
    '--line': '#e6e3d8', '--border': '#e6e3d8', '--border-strong': '#d5d1c2',
    '--shadow-sm': '0 1px 2px rgba(61,57,41,.06)',
    '--shadow': '0 2px 8px rgba(61,57,41,.08)',
    '--shadow-lg': '0 10px 32px rgba(61,57,41,.12)',
    '--bg': '#faf9f5', '--surface': '#ffffff', '--surface-2': '#f5f4ef', '--surface-3': '#ede9de',
    '--paper-1': '#f5f4ef', '--muted': '#6e6d68',
    '--text': '#3d3929', '--text-2': '#535146', '--text-3': '#6e6d68',
    '--accent': '#c96442', '--accent-strong': '#934828', '--accent-soft': '#f4e0d5', '--accent-bg': '#fbf2ed',
    '--risk-safe': '#3e7a52', '--risk-caution': '#a56a2e', '--risk-danger': '#b0503a',
    '--radius': '1rem',
  },
  // ── Google ─────────────────────────────────────────────────────────────
  // Clean analytical: #fff / #0e1115 / card-foreground #0e1115, muted
  // #eff1f4, border #e7eaef, brand Google-blue #1a73e8 primary, radius 2rem
  // (Material friendliness, softened to 1rem for a dense tool).
  google: {
    '--ink': '#0e1115', '--ink-2': '#3c4043', '--ink-3': '#70757a',
    '--paper': '#ffffff', '--paper-2': '#f8f9fa', '--paper-3': '#f1f3f4', '--card': '#ffffff',
    '--pink': '#1a73e8', '--pink-deep': '#165dbc', '--pink-soft': '#d6e5f8', '--pink-bg': '#e8f0fe',
    '--lavender': '#9a8fd9', '--mint': '#55b68d', '--peach': '#f2a776', '--sun': '#e5c158',
    '--ok': '#188038', '--warn': '#b06000', '--err': '#d93025',
    '--line': '#e0e3e6', '--border': '#e0e3e6', '--border-strong': '#ccd1d5',
    '--shadow-sm': '0 1px 2px rgba(14,17,21,.05)',
    '--shadow': '0 1px 3px rgba(14,17,21,.06), 0 2px 8px rgba(14,17,21,.05)',
    '--shadow-lg': '0 4px 16px rgba(14,17,21,.08)',
    '--bg': '#ffffff', '--surface': '#ffffff', '--surface-2': '#f8f9fa', '--surface-3': '#f1f3f4',
    '--paper-1': '#f8f9fa', '--muted': '#70757a',
    '--text': '#0e1115', '--text-2': '#3c4043', '--text-3': '#70757a',
    '--accent': '#1a73e8', '--accent-strong': '#165dbc', '--accent-soft': '#d6e5f8', '--accent-bg': '#e8f0fe',
    '--risk-safe': '#188038', '--risk-caution': '#b06000', '--risk-danger': '#d93025',
    '--radius': '1rem',
  },
  // ── Volcengine ──────────────────────────────────────────────────────────
  // Light: #fff, primary yuanliBlue-6 #1664ff, muted #86909c, border #e5e8f0.
  // Dark: bg #0c0d0e, card #1d2129, popover #2a3440, border #333333, primary
  // #1664ff (kept for 'pro', dark below).
  volcengine: {
    '--ink': '#1d2129', '--ink-2': '#4e5969', '--ink-3': '#86909c',
    '--paper': '#fafbfc', '--paper-2': '#f2f3f5', '--paper-3': '#e5e8f0', '--card': '#ffffff',
    '--pink': '#1664ff', '--pink-deep': '#0040c2', '--pink-soft': '#dbe7ff', '--pink-bg': '#f3f7ff',
    '--lavender': '#9a90e0', '--mint': '#4fc3a1', '--peach': '#f2a05e', '--sun': '#e5c158',
    '--ok': '#0f9b62', '--warn': '#c28300', '--err': '#e02e1f',
    '--line': '#e5e8f0', '--border': '#e5e8f0', '--border-strong': '#cdd3e0',
    '--shadow-sm': '0 1px 2px rgba(14,21,38,.05)',
    '--shadow': '0 2px 8px rgba(14,21,38,.07)',
    '--shadow-lg': '0 8px 24px rgba(14,21,38,.1)',
    '--bg': '#fafbfc', '--surface': '#ffffff', '--surface-2': '#f2f3f5', '--surface-3': '#e5e8f0',
    '--paper-1': '#f2f3f5', '--muted': '#86909c',
    '--text': '#1d2129', '--text-2': '#4e5969', '--text-3': '#86909c',
    '--accent': '#1664ff', '--accent-strong': '#0040c2', '--accent-soft': '#dbe7ff', '--accent-bg': '#f3f7ff',
    '--risk-safe': '#0f9b62', '--risk-caution': '#c28300', '--risk-danger': '#e02e1f',
  },
  // ── Nerv ────────────────────────────────────────────────────────────────
  // Dark neon editorial, high contrast: bg rgb(15,15,16), card rgb(17,17,18),
  // foreground #f4f9ff, primary rgb(234,52,58) signal red, accent slate,
  // ring #f4f9ff. radius 1.65rem (clamped for tool density).
  nerv: {
    '--ink': '#f4f9ff', '--ink-2': '#c9d2da', '--ink-3': '#8a93a0',
    '--paper': '#0f0f10', '--paper-2': '#111112', '--paper-3': '#19191b', '--card': '#111112',
    '--pink': '#ea343a', '--pink-deep': '#ff5c61', '--pink-soft': '#3f1a1c', '--pink-bg': '#241114',
    '--lavender': '#a083e8', '--mint': '#4fd0a0', '--peach': '#f0a35e', '--sun': '#e5c758',
    '--ok': '#3ecf8e', '--warn': '#f5b071', '--err': '#ff3434',
    '--line': '#303136', '--border': '#303136', '--border-strong': '#44454c',
    '--shadow-sm': '0 1px 2px rgba(0,0,0,.5)',
    '--shadow': '0 4px 12px rgba(0,0,0,.5)',
    '--shadow-lg': '0 10px 30px rgba(0,0,0,.55)',
    '--bg': '#0f0f10', '--surface': '#17171a', '--surface-2': '#111112', '--surface-3': '#19191b',
    '--paper-1': '#111112', '--muted': '#8a93a0',
    '--text': '#f4f9ff', '--text-2': '#c9d2da', '--text-3': '#8a93a0',
    '--accent': '#ea343a', '--accent-strong': '#ff5c61', '--accent-soft': '#3f1a1c', '--accent-bg': '#241114',
    '--risk-safe': '#3ecf8e', '--risk-caution': '#f5b071', '--risk-danger': '#ff3434',
  },
  // ── TRAE Work ───────────────────────────────────────────────────────────
  // Engineering studio: #fff/#f5f5f5/#e5e5e5 surfaces (bg-base default/secondary/tertiary),
  // text-700 #171717 / text-secondary #404040 / text-tertiary #737373,
  // border rgba(115,115,115,.12)/.18/.36 (neutral-l1/l2/l3 → border/strong/stronger),
  // brand #4B3FE3 violet, accent-teal #00B983 / accent-coral #FF6B45 / accent-amber #F2A90C.
  trae: {
    '--ink': '#171717', '--ink-2': '#404040', '--ink-3': '#737373',
    '--paper': '#ffffff', '--paper-2': '#f5f5f5', '--paper-3': '#e5e5e5', '--card': '#ffffff',
    '--pink': '#4b3fe3', '--pink-deep': '#312994', '--pink-soft': '#e7e6fd', '--pink-bg': '#f1f0fe',
    '--lavender': '#6a6fff', '--mint': '#00b983', '--peach': '#ff6b45', '--sun': '#f2a90c',
    '--ok': '#00b983', '--warn': '#d98a00', '--err': '#f03d3d',
    '--line': '#e2e2e5', '--border': '#e2e2e5', '--border-strong': '#ccccd1',
    '--shadow-sm': '0 1px 2px rgba(0,0,0,.05)',
    '--shadow': '0 2px 6px rgba(0,0,0,.06), 0 4px 14px rgba(0,0,0,.05)',
    '--shadow-lg': '0 8px 28px rgba(0,0,0,.1)',
    '--bg': '#ffffff', '--surface': '#ffffff', '--surface-2': '#f5f5f5', '--surface-3': '#e5e5e5',
    '--paper-1': '#f5f5f5', '--muted': '#737373',
    '--text': '#171717', '--text-2': '#404040', '--text-3': '#737373',
    '--accent': '#4b3fe3', '--accent-strong': '#312994', '--accent-soft': '#e7e6fd', '--accent-bg': '#f1f0fe',
    '--risk-safe': '#00b983', '--risk-caution': '#d98a00', '--risk-danger': '#f03d3d',
  },
  // ── Motion Fit ──────────────────────────────────────────────────────────
  // Dark energetic: bg #292929, card/sidebar #030303, foreground #e2e8f0,
  // primary/signal #ff4000, accent #737373, border #292929, neon chart ramps.
  motionfit: {
    '--ink': '#e2e8f0', '--ink-2': '#cbd5e1', '--ink-3': '#94a3b8',
    '--paper': '#292929', '--paper-2': '#17181a', '--paper-3': '#111112', '--card': '#030303',
    '--pink': '#ff4000', '--pink-deep': '#ff7a4d', '--pink-soft': '#571a03', '--pink-bg': '#2b1608',
    '--lavender': '#a083e8', '--mint': '#00ff1e', '--peach': '#ff3f02', '--sun': '#d6ff0a',
    '--ok': '#00ff1e', '--warn': '#f5b071', '--err': '#ef4444',
    '--line': '#3a3a3c', '--border': '#3a3a3c', '--border-strong': '#4a4a4c',
    '--shadow-sm': '0 1px 2px rgba(0,0,0,.5)',
    '--shadow': '0 4px 12px rgba(0,0,0,.5)',
    '--shadow-lg': '0 10px 30px rgba(0,0,0,.55)',
    '--bg': '#292929', '--surface': '#17181a', '--surface-2': '#111112', '--surface-3': '#0c0d0e',
    '--paper-1': '#17181a', '--muted': '#94a3b8',
    '--text': '#e2e8f0', '--text-2': '#cbd5e1', '--text-3': '#94a3b8',
    '--accent': '#ff4000', '--accent-strong': '#ff7a4d', '--accent-soft': '#571a03', '--accent-bg': '#2b1608',
    '--risk-safe': '#00ff1e', '--risk-caution': '#f5b071', '--risk-danger': '#ef4444',
  },
  // ── Barbie ──────────────────────────────────────────────────────────────
  // Playful bubblegum: #fff background, card #fce8f1 (pink-tinted), foreground
  // #381422, primary #e11d48 rose-600, secondary/muted pink, radius 1.8rem.
  barbie: {
    '--ink': '#381422', '--ink-2': '#741d3d', '--ink-3': '#a85d76',
    '--paper': '#fff5f9', '--paper-2': '#fce8f1', '--paper-3': '#f9d5e6', '--card': '#ffffff',
    '--pink': '#e11d48', '--pink-deep': '#9e1432', '--pink-soft': '#ffafcc', '--pink-bg': '#ffe3ef',
    '--lavender': '#e59bc8', '--mint': '#8fdcb0', '--peach': '#ffb37a', '--sun': '#ffd85e',
    '--ok': '#c2577a', '--warn': '#d97a3e', '--err': '#d61f3f',
    '--line': '#f3c7d8', '--border': '#f3c7d8', '--border-strong': '#e6a9c0',
    '--shadow-sm': '0 2px 6px rgba(158,20,50,.06)',
    '--shadow': '0 4px 14px rgba(158,20,50,.08)',
    '--shadow-lg': '0 10px 30px rgba(158,20,50,.12)',
    '--bg': '#fff5f9', '--surface': '#ffffff', '--surface-2': '#fce8f1', '--surface-3': '#f9d5e6',
    '--paper-1': '#fce8f1', '--muted': '#a85d76',
    '--text': '#381422', '--text-2': '#741d3d', '--text-3': '#a85d76',
    '--accent': '#e11d48', '--accent-strong': '#9e1432', '--accent-soft': '#ffafcc', '--accent-bg': '#ffe3ef',
    '--risk-safe': '#1f9d6a', '--risk-caution': '#d97706', '--risk-danger': '#d61f3f',
    '--radius': '1.8rem',
  },
  // ── Apple Copy (Pinguo) ─────────────────────────────────────────────────
  // Restrained elegant, Apple-inspired: brand-500 #007aff iOS blue, gray
  // ramps (background-50 #fff → 200 #f2f2f7), radius 2rem (light/filled
  // UI). Rounded pill surfaces, soft borders.
  brand: {
    '--ink': '#1c1c1e', '--ink-2': '#3c3c43', '--ink-3': '#8e8e93',
    '--paper': '#f7f7fa', '--paper-2': '#f2f2f7', '--paper-3': '#e5e5ea', '--card': '#ffffff',
    '--pink': '#007aff', '--pink-deep': '#0055b3', '--pink-soft': '#d6e6ff', '--pink-bg': '#eef4ff',
    '--lavender': '#9a8fd9', '--mint': '#55b68d', '--peach': '#f2a776', '--sun': '#e5c158',
    '--ok': '#188038', '--warn': '#b06000', '--err': '#d93025',
    '--line': '#dddddf', '--border': '#dddddf', '--border-strong': '#c6c6cb',
    '--shadow-sm': '0 1px 2px rgba(28,28,30,.05)',
    '--shadow': '0 1px 3px rgba(28,28,30,.06), 0 2px 10px rgba(28,28,30,.06)',
    '--shadow-lg': '0 4px 20px rgba(28,28,30,.09)',
    '--bg': '#f7f7fa', '--surface': '#ffffff', '--surface-2': '#f2f2f7', '--surface-3': '#e5e5ea',
    '--paper-1': '#f2f2f7', '--muted': '#8e8e93',
    '--text': '#1c1c1e', '--text-2': '#3c3c43', '--text-3': '#6b6b70',
    '--accent': '#007aff', '--accent-strong': '#0055b3', '--accent-soft': '#d6e6ff', '--accent-bg': '#eef4ff',
    '--risk-safe': '#188038', '--risk-caution': '#b06000', '--risk-danger': '#d93025',
    '--radius': '2rem',
  },
  // ── Golden Time ─────────────────────────────────────────────────────────
  // Warm editorial: primary #5a4f43 espresso, secondary #b98a4f caramel-gold,
  // accent #d4c7a4 sand, bg #f8f4ec cream, border #ece4d4, radius 2rem
  // (softened to 1.2rem for dense tool rows).
  golden: {
    '--ink': '#4d4338', '--ink-2': '#6f6357', '--ink-3': '#8a7d6f',
    '--paper': '#f8f4ec', '--paper-2': '#f1eadd', '--paper-3': '#e8ddcc', '--card': '#fffdf8',
    '--pink': '#b98a4f', '--pink-deep': '#8a6136', '--pink-soft': '#ecd9b6', '--pink-bg': '#f6ecdb',
    '--lavender': '#9a8fd9', '--mint': '#7fb389', '--peach': '#d99b6a', '--sun': '#c9a95d',
    '--ok': '#3e7a52', '--warn': '#a56a2e', '--err': '#a83a2e',
    '--line': '#ece4d4', '--border': '#ece4d4', '--border-strong': '#ddcfb4',
    '--shadow-sm': '0 1px 2px rgba(77,67,56,.06)',
    '--shadow': '0 2px 8px rgba(77,67,56,.08)',
    '--shadow-lg': '0 10px 30px rgba(77,67,56,.12)',
    '--bg': '#f8f4ec', '--surface': '#fffdf8', '--surface-2': '#f1eadd', '--surface-3': '#e8ddcc',
    '--paper-1': '#f1eadd', '--muted': '#8a7d6f',
    '--text': '#4d4338', '--text-2': '#6f6357', '--text-3': '#8a7d6f',
    '--accent': '#b98a4f', '--accent-strong': '#8a6136', '--accent-soft': '#ecd9b6', '--accent-bg': '#f6ecdb',
    '--risk-safe': '#3e7a52', '--risk-caution': '#a56a2e', '--risk-danger': '#a83a2e',
    '--radius': '1.2rem',
  },
  // ── Vibe Camp ───────────────────────────────────────────────────────────
  // Terracotta clay: bg #e3dfd9 warm stone, card #f3f0eb raised cream,
  // foreground #211d1a coffee, muted #5c5650, border #cbc3ba, primary
  // brand-600 #f1481e terracotta, radius 18px (softened to 1rem).
  vibecamp: {
    '--ink': '#211d1a', '--ink-2': '#423c36', '--ink-3': '#5c5650',
    '--paper': '#e3dfd9', '--paper-2': '#ded9d2', '--paper-3': '#d3ccc3', '--card': '#f3f0eb',
    '--pink': '#f1481e', '--pink-deep': '#ad472a', '--pink-soft': '#f8c9b8', '--pink-bg': '#fce8e0',
    '--lavender': '#9a8fd9', '--mint': '#6fae7d', '--peach': '#f0a35e', '--sun': '#d4b45c',
    '--ok': '#3e7a52', '--warn': '#a56a2e', '--err': '#d64527',
    '--line': '#cbc3ba', '--border': '#cbc3ba', '--border-strong': '#b0a79d',
    '--shadow-sm': '0 1px 2px rgba(33,29,26,.08)',
    '--shadow': '0 2px 8px rgba(33,29,26,.1)',
    '--shadow-lg': '0 10px 28px rgba(33,29,26,.14)',
    '--bg': '#e3dfd9', '--surface': '#f3f0eb', '--surface-2': '#ded9d2', '--surface-3': '#d3ccc3',
    '--paper-1': '#ded9d2', '--muted': '#5c5650',
    '--text': '#211d1a', '--text-2': '#423c36', '--text-3': '#5c5650',
    '--accent': '#f1481e', '--accent-strong': '#ad472a', '--accent-soft': '#f8c9b8', '--accent-bg': '#fce8e0',
    '--risk-safe': '#3e7a52', '--risk-caution': '#a56a2e', '--risk-danger': '#d64527',
    '--radius': '1rem',
  },
  // ── 21st ────────────────────────────────────────────────────────────────
  // macOS graphite minimal: bg #d8dadf (soft cool gray), card #e2e4e8,
  // foreground #2d2d2f, primary near-black #232327, accent indigo #5262e8,
  // radius 0 (square, Apple-flat).
  '21th': {
    '--ink': '#2d2d2f', '--ink-2': '#535358', '--ink-3': '#76767c',
    '--paper': '#d8dadf', '--paper-2': '#e2e4e8', '--paper-3': '#c9ccd2', '--card': '#e2e4e8',
    '--pink': '#232327', '--pink-deep': '#4d4d55', '--pink-soft': '#c3c7ce', '--pink-bg': '#d0d3d9',
    '--lavender': '#6a6ff0', '--mint': '#55b68d', '--peach': '#f2a776', '--sun': '#d6b45c',
    '--ok': '#2f9e63', '--warn': '#b07a2e', '--err': '#d64527',
    '--line': '#c1c4cb', '--border': '#c1c4cb', '--border-strong': '#a9adb5',
    '--shadow-sm': '0 1px 2px rgba(45,45,47,.1)',
    '--shadow': '0 2px 8px rgba(45,45,47,.12)',
    '--shadow-lg': '0 10px 28px rgba(45,45,47,.16)',
    '--bg': '#d8dadf', '--surface': '#e2e4e8', '--surface-2': '#c9ccd2', '--surface-3': '#bec1c8',
    '--paper-1': '#c9ccd2', '--muted': '#76767c',
    '--text': '#2d2d2f', '--text-2': '#535358', '--text-3': '#76767c',
    '--accent': '#5262e8', '--accent-strong': '#3f4ec0', '--accent-soft': '#d3d8f8', '--accent-bg': '#e6e9fb',
    '--risk-safe': '#2f9e63', '--risk-caution': '#b07a2e', '--risk-danger': '#d64527',
    '--radius': '0',
  },
  // ── Steam-ish (dwarven tech green, dark) ───────────────────────────────
  // Dark #1b2838 deep slate bg, #171a21 furnace card, valve-green #66c0f4
  // primary, muted #8f98a0, border #4b5664, radius 1rem. Warm, gamer-tech.
  steam: {
    '--ink': '#e9eef4', '--ink-2': '#b3bcc6', '--ink-3': '#8f98a0',
    '--paper': '#1b2838', '--paper-2': '#212e40', '--paper-3': '#2a3a50', '--card': '#171a21',
    '--pink': '#66c0f4', '--pink-deep': '#a4d7f9', '--pink-soft': '#1e3a52', '--pink-bg': '#14222e',
    '--lavender': '#a083e8', '--mint': '#6fd4b3', '--peach': '#f0a35e', '--sun': '#d4c46a',
    '--ok': '#7ecba1', '--warn': '#f5b071', '--err': '#ff6d85',
    '--line': '#4b5664', '--border': '#4b5664', '--border-strong': '#5d6a7a',
    '--shadow-sm': '0 1px 2px rgba(0,0,0,.4)',
    '--shadow': '0 4px 12px rgba(0,0,0,.45)',
    '--shadow-lg': '0 10px 30px rgba(0,0,0,.5)',
    '--bg': '#1b2838', '--surface': '#171a21', '--surface-2': '#212e40', '--surface-3': '#2a3a50',
    '--paper-1': '#212e40', '--muted': '#8f98a0',
    '--text': '#e9eef4', '--text-2': '#b3bcc6', '--text-3': '#8f98a0',
    '--accent': '#66c0f4', '--accent-strong': '#a4d7f9', '--accent-soft': '#1e3a52', '--accent-bg': '#14222e',
    '--risk-safe': '#7ecba1', '--risk-caution': '#f5b071', '--risk-danger': '#ff6d85',
  },
  // ── Dark (Nerv dark, compatibility alias) ───────────────────────────────
  dark: {
    '--ink': '#f4f9ff', '--ink-2': '#b8b4c4', '--ink-3': '#8a93a0',
    '--paper': '#0f0f10', '--paper-2': '#17171a', '--paper-3': '#1c1c1f', '--card': '#111112',
    '--pink': '#ea343a', '--pink-deep': '#ff5c61', '--pink-soft': '#3f1a1c', '--pink-bg': '#241114',
    '--lavender': '#a083e8', '--mint': '#4fd0a0', '--peach': '#f0a35e', '--sun': '#e5c758',
    '--ok': '#6fd4a0', '--warn': '#f5b071', '--err': '#ff6d85',
    '--line': '#303136', '--border': '#303136', '--border-strong': '#44454c',
    '--shadow-sm': '0 1px 2px rgba(0,0,0,.5)',
    '--shadow': '0 4px 12px rgba(0,0,0,.5)',
    '--shadow-lg': '0 10px 30px rgba(0,0,0,.55)',
    '--bg': '#0f0f10', '--surface': '#17171a', '--surface-2': '#111112', '--surface-3': '#19191b',
    '--paper-1': '#17171a', '--muted': '#8a93a0',
    '--text': '#f4f9ff', '--text-2': '#c9d2da', '--text-3': '#8a93a0',
    '--accent': '#ea343a', '--accent-strong': '#ff5c61', '--accent-soft': '#3f1a1c', '--accent-bg': '#241114',
    '--risk-safe': '#6fd4a0', '--risk-caution': '#f5b071', '--risk-danger': '#ff6d85',
  },
  // ── Pro (Volcengine dark, compatibility alias) ─────────────────────────
  pro: {
    '--ink': '#ffffff', '--ink-2': '#c9cdd4', '--ink-3': '#7d899e',
    '--paper': '#0c0d0e', '--paper-2': '#1d2129', '--paper-3': '#2a3440', '--card': '#1d2129',
    '--pink': '#1664ff', '--pink-deep': '#4d8dff', '--pink-soft': '#0f2440', '--pink-bg': '#101a30',
    '--lavender': '#8ab4f8', '--mint': '#6fd4a0', '--peach': '#f0b285', '--sun': '#f0d878',
    '--ok': '#6fd4a0', '--warn': '#f5b071', '--err': '#ff7d8a',
    '--line': '#333333', '--border': '#333333', '--border-strong': '#41464f',
    '--shadow-sm': '0 1px 2px rgba(0,0,0,.4)',
    '--shadow': '0 4px 12px rgba(0,0,0,.45)',
    '--shadow-lg': '0 10px 30px rgba(0,0,0,.5)',
    '--bg': '#0c0d0e', '--surface': '#1d2129', '--surface-2': '#1d2129', '--surface-3': '#2a3440',
    '--paper-1': '#1d2129', '--muted': '#7d899e',
    '--text': '#ffffff', '--text-2': '#c9cdd4', '--text-3': '#7d899e',
    '--accent': '#1664ff', '--accent-strong': '#4d8dff', '--accent-soft': '#0f2440', '--accent-bg': '#101a30',
    '--risk-safe': '#6fd4a0', '--risk-caution': '#f5b071', '--risk-danger': '#ff7d8a',
  },
};

const ALL_VARS = Array.from(new Set(
  Object.values(PRESETS).flatMap((p) => Object.keys(p))
    .concat([
      '--pink', '--pink-deep', '--pink-soft', '--pink-bg', '--ok',
      '--bg', '--surface', '--surface-2', '--surface-3', '--paper-1', '--muted',
      '--text', '--text-2', '--text-3',
      '--accent', '--accent-strong', '--accent-soft', '--accent-bg',
      '--risk-safe', '--risk-caution', '--risk-danger',
      '--border', '--border-strong', '--radius',
      '--radius-xs', '--radius-sm', '--radius-btn', '--radius-md', '--radius-card', '--radius-lg',
      '--font-scale', '--font-sans', '--font-mono',
    ]),
));

// ── hex color helpers ─────────────────────────────────────────────────────
function hexToRgb(h: string): [number, number, number] {
  let s = h.trim().replace(/^#/, '');
  if (s.length === 3) s = s.split('').map((c) => c + c).join('');
  const n = Number.parseInt(s, 16);
  if (!Number.isFinite(n) || s.length !== 6) return [128, 128, 128];
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}
function rgbToHex(r: number, g: number, b: number): string {
  const c = (x: number) => Math.max(0, Math.min(255, Math.round(x))).toString(16).padStart(2, '0');
  return `#${c(r)}${c(g)}${c(b)}`;
}
/** Mix toward white (t>0) / black (t<0). */
function shade(h: string, t: number): string {
  const [r, g, b] = hexToRgb(h);
  const target = t >= 0 ? 255 : 0;
  const k = Math.abs(t);
  return rgbToHex(r + (target - r) * k, g + (target - g) * k, b + (target - b) * k);
}
/** Rough perceived brightness 0..1 — dark presets need lightened accents. */
function brightness(h: string): number {
  const [r, g, b] = hexToRgb(h);
  return (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
}

const DARK_IDS: ReadonlySet<ThemeId> = new Set<ThemeId>([
  'nerv', 'motionfit', 'steam', 'dark', 'pro',
] as ThemeId[]);

function applyAccent(el: CSSStyleDeclaration, accent: string, themeId: ThemeId) {
  const dark = DARK_IDS.has(themeId);
  const deep = dark ? shade(accent, 0.45) : shade(accent, -0.28);
  const soft = dark ? shade(accent, -0.45) : shade(accent, 0.55);
  const bg = dark ? shade(accent, -0.7) : shade(accent, 0.84);
  el.setProperty('--pink', accent);
  // --pink-deep is used as TEXT color; on dark it must go lighter, else it
  // sinks into the background.
  el.setProperty('--pink-deep', deep);
  el.setProperty('--pink-soft', soft);
  el.setProperty('--pink-bg', bg);
  // 语义层跟随自定义主色（W4）：组件从 --accent 系取值时也能吃到自定义色
  el.setProperty('--accent', accent);
  el.setProperty('--accent-strong', deep);
  el.setProperty('--accent-soft', soft);
  el.setProperty('--accent-bg', bg);
  // Status "ok" follows the accent so custom-colored themes feel coherent —
  // but only if the accent is bright enough to act as a button fill.
  if (brightness(accent) > 0.25) {
    el.setProperty('--ok', accent);
    el.setProperty('--risk-safe', accent);
  }
}

export function applyTheme(t: ThemeSettings) {
  const el = document.documentElement.style;
  for (const k of ALL_VARS) el.removeProperty(k);
  const preset = PRESETS[t.id] ?? PRESETS.vercel;
  for (const [k, v] of Object.entries(preset)) el.setProperty(k, v);
  if (t.accent && /^#[0-9a-fA-F]{6}$/.test(t.accent)) applyAccent(el, t.accent, t.id);
  const scale = Math.max(0.8, Math.min(1.3, t.fontScale || 1));
  el.setProperty('--font-scale', String(scale));
  document.documentElement.setAttribute('data-theme', t.id);
  // Fonts follow the kit's type voice.
  const fonts = THEME_FONTS[t.id] ?? THEME_FONTS.vercel;
  el.setProperty('--font-sans', fonts.ui);
  el.setProperty('--font-mono', fonts.editor);
  document.documentElement.style.colorScheme = DARK_IDS.has(t.id) ? 'dark' : 'light';
}

const VALID_IDS = new Set<string>(Object.keys(PRESETS));

const FALLBACK: ThemeSettings = { id: 'vercel', accent: null, fontScale: 1 };

export function loadTheme(): ThemeSettings {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return { ...FALLBACK };
    const p = JSON.parse(raw) as Partial<ThemeSettings>;
    return {
      id: p.id && VALID_IDS.has(p.id) ? (p.id as ThemeId) : 'vercel',
      accent: typeof p.accent === 'string' && /^#[0-9a-fA-F]{6}$/.test(p.accent) ? p.accent : null,
      fontScale: Number.isFinite(p.fontScale) ? (p.fontScale as number) : 1,
    };
  } catch {
    return { ...FALLBACK };
  }
}

export function saveTheme(t: ThemeSettings) {
  try { localStorage.setItem(KEY, JSON.stringify(t)); } catch { /* ignore */ }
  applyTheme(t);
  window.dispatchEvent(new CustomEvent('diskpilot:theme-changed', { detail: t }));
}

/** Called once from main.tsx before render — no theme flash on startup. */
export function initTheme() {
  applyTheme(loadTheme());
}

// ── 通用偏好（设置页「通用」标签写，AI 链路读）──────────────────────────
export const PREF_KEYS = {
  webEnabled: 'diskpilot.agent.web',
  showThinking: 'diskpilot.agent.thinking',
  autoOverview: 'diskpilot.general.autoOverview',
  parallelScan: 'diskpilot.general.parallelScan',
  hardwareAccel: 'diskpilot.general.hardwareAccel',
  keepFilesPerDir: 'diskpilot.general.keepFilesPerDir',
} as const;

export type PrefName = keyof typeof PREF_KEYS;

function prefGet(name: PrefName): boolean {
  try { return localStorage.getItem(PREF_KEYS[name]) !== '0'; } catch { return true; }
}
export function setPref(name: PrefName, on: boolean) {
  try { localStorage.setItem(PREF_KEYS[name], on ? '1' : '0'); } catch { /* ignore */ }
}

/** Number preference: 每目录保留文件数（默认 500）。非法值回落默认。 */
const KEEP_FILES_KEY = PREF_KEYS.keepFilesPerDir;
export function getKeepFiles(): number {
  try {
    const v = Number(localStorage.getItem(KEEP_FILES_KEY));
    if (Number.isFinite(v) && v >= 50 && v <= 2000) return v;
  } catch { /* ignore */ }
  return 500;
}
export function setKeepFiles(v: number) {
  try { localStorage.setItem(KEEP_FILES_KEY, String(v)); } catch { /* ignore */ }
}

export const prefs = {
  get webEnabled() { return prefGet('webEnabled'); },
  get showThinking() { return prefGet('showThinking'); },
  get autoOverview() { return prefGet('autoOverview'); },
  get parallelScan() { return prefGet('parallelScan'); },
  get hardwareAccel() { return prefGet('hardwareAccel'); },
  get keepFilesPerDir() { return getKeepFiles(); },
};