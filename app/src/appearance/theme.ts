import {
  converter,
  formatRgb,
  interpolate,
  modeLrgb,
  modeOklch,
  modeRgb,
  useMode,
  wcagContrast,
} from "culori/fn"

const SYSTEM_COLOR_SCHEME_MEDIA_QUERY = "(prefers-color-scheme: dark)"

useMode(modeRgb)
useMode(modeOklch)
useMode(modeLrgb)

const toRgb = converter("rgb")

/** Built-in appearance modes supported by the app shell. */
export type AppearanceMode = "system" | "light" | "dark"

/** One resolved built-in theme name after system preference fallback is applied. */
export type BuiltInThemeName = "light" | "dark"

/** Persisted appearance preferences plus the currently observed system theme. */
export type AppearanceSnapshot = {
  mode: AppearanceMode
  highContrast: boolean
  systemTheme: BuiltInThemeName
}

/** One resolved theme application payload for document attributes and CSS variables. */
export type AppearanceDocumentState = {
  themeName: BuiltInThemeName
  attributes: Record<string, string>
  variables: Record<string, string>
}

type OklchColor = {
  mode: "oklch"
  l: number
  c: number
  h: number
  alpha?: number
}

type ThemeSeeds = {
  accent: OklchColor
  background: OklchColor
  foreground: OklchColor
  danger: OklchColor
}

const builtInThemeSeeds = {
  light: {
    accent: { mode: "oklch", l: 0.64, c: 0.11, h: 250 },
    background: { mode: "oklch", l: 0.985, c: 0.006, h: 252 },
    foreground: { mode: "oklch", l: 0.245, c: 0.02, h: 252 },
    danger: { mode: "oklch", l: 0.62, c: 0.18, h: 25 },
  },
  dark: {
    accent: { mode: "oklch", l: 0.74, c: 0.11, h: 250 },
    background: { mode: "oklch", l: 0.2, c: 0.008, h: 252 },
    foreground: { mode: "oklch", l: 0.94, c: 0.015, h: 252 },
    danger: { mode: "oklch", l: 0.7, c: 0.16, h: 25 },
  },
} satisfies Record<BuiltInThemeName, ThemeSeeds>

function clamp(value: number, minimum: number, maximum: number) {
  return Math.min(maximum, Math.max(minimum, value))
}

function mixColors(first: OklchColor, second: OklchColor, amount: number) {
  const mix = interpolate([first, second], "oklch")
  return (mix(clamp(amount, 0, 1)) as OklchColor | undefined) ?? second
}

function withLightness(color: OklchColor, delta: number) {
  return {
    ...color,
    l: clamp(color.l + delta, 0, 1),
  }
}

function withChroma(color: OklchColor, factor: number) {
  return {
    ...color,
    c: clamp(color.c * factor, 0, 0.4),
  }
}

function withAlpha(color: OklchColor, alpha: number) {
  return {
    ...color,
    alpha: clamp(alpha, 0, 1),
  }
}

function ensureContrast(
  color: OklchColor,
  against: OklchColor,
  toward: OklchColor,
  minimumContrast: number,
) {
  if (wcagContrast(color, against) >= minimumContrast) {
    return color
  }

  for (let step = 1; step <= 14; step += 1) {
    const candidate = mixColors(color, toward, step / 14)

    if (wcagContrast(candidate, against) >= minimumContrast) {
      return candidate
    }
  }

  return toward
}

function chooseOnAccent(accent: OklchColor, background: OklchColor, foreground: OklchColor) {
  return wcagContrast(accent, foreground) >= wcagContrast(accent, background)
    ? foreground
    : background
}

function serializeColor(color: OklchColor) {
  return formatRgb(toRgb(color) ?? color)
}

/** Returns whether one runtime value is a supported appearance mode. */
export function isAppearanceMode(value: unknown): value is AppearanceMode {
  return value === "system" || value === "light" || value === "dark"
}

function deriveThemeVariables(themeName: BuiltInThemeName, highContrast: boolean) {
  const seeds = builtInThemeSeeds[themeName]
  const isDark = themeName === "dark"
  const contrastStrength = highContrast ? 1.45 : 1
  const background = seeds.background
  const foreground = seeds.foreground
  const surface = mixColors(background, foreground, (isDark ? 0.042 : 0.03) * contrastStrength)
  const panel = mixColors(background, foreground, (isDark ? 0.078 : 0.052) * contrastStrength)
  const border = ensureContrast(
    mixColors(background, foreground, (isDark ? 0.18 : 0.14) * contrastStrength),
    background,
    foreground,
    highContrast ? 2.6 : 1.8,
  )
  const muted = ensureContrast(
    mixColors(foreground, background, highContrast ? 0.2 : 0.34),
    background,
    foreground,
    highContrast ? 6.2 : 4.7,
  )
  const accent = ensureContrast(
    withChroma(seeds.accent, highContrast ? 1.1 : 1),
    background,
    foreground,
    highContrast ? 3.6 : 2.8,
  )
  const accentStrong = ensureContrast(
    mixColors(withLightness(accent, isDark ? 0.04 : -0.04), foreground, isDark ? 0.06 : 0.12),
    background,
    foreground,
    highContrast ? 4.8 : 3.8,
  )
  const focus = accentStrong
  const accentFg = chooseOnAccent(accentStrong, background, foreground)
  const danger = ensureContrast(
    withChroma(seeds.danger, highContrast ? 1.08 : 1),
    background,
    foreground,
    highContrast ? 4.5 : 3.4,
  )
  const overlay = withAlpha(mixColors(foreground, background, isDark ? 0.18 : 0.42), 0.52)
  const shadow = withAlpha(mixColors(foreground, background, isDark ? 0.08 : 0.6), 0.18)
  const transcriptGlowBase = mixColors(accent, background, isDark ? 0.54 : 0.78)
  const transcriptGlow = withAlpha(transcriptGlowBase, isDark ? 0.26 : 0.16)
  const transcriptGlowEdge = withAlpha(transcriptGlowBase, 0)
  const transcriptUserBubbleStart = mixColors(
    panel,
    withChroma(accentStrong, highContrast ? 0.98 : 0.78),
    isDark ? 0.34 : 0.24,
  )
  const transcriptUserBubbleEnd = mixColors(
    surface,
    withChroma(accent, highContrast ? 0.94 : 0.74),
    isDark ? 0.24 : 0.16,
  )
  const transcriptUserBubbleBorder = ensureContrast(
    mixColors(transcriptUserBubbleStart, accentStrong, isDark ? 0.42 : 0.52),
    background,
    foreground,
    highContrast ? 2.6 : 1.8,
  )
  const transcriptUserCodeBg = mixColors(
    transcriptUserBubbleStart,
    accentStrong,
    isDark ? 0.16 : 0.12,
  )
  const transcriptUserCodeBorder = ensureContrast(
    mixColors(transcriptUserBubbleBorder, accentStrong, isDark ? 0.2 : 0.18),
    background,
    foreground,
    highContrast ? 2.9 : 2,
  )

  return {
    "--theme-color-background": serializeColor(background),
    "--theme-color-surface": serializeColor(surface),
    "--theme-color-panel": serializeColor(panel),
    "--theme-color-border": serializeColor(border),
    "--theme-color-text": serializeColor(foreground),
    "--theme-color-muted": serializeColor(muted),
    "--theme-color-focus": serializeColor(focus),
    "--theme-color-accent": serializeColor(accent),
    "--theme-color-accent-strong": serializeColor(accentStrong),
    "--theme-color-accent-fg": serializeColor(accentFg),
    "--theme-color-danger": serializeColor(danger),
    "--theme-color-overlay": serializeColor(overlay),
    "--theme-color-shadow": serializeColor(shadow),
    "--theme-color-transcript-glow": serializeColor(transcriptGlow),
    "--theme-color-transcript-glow-edge": serializeColor(transcriptGlowEdge),
    "--theme-color-transcript-user-bubble-start": serializeColor(transcriptUserBubbleStart),
    "--theme-color-transcript-user-bubble-end": serializeColor(transcriptUserBubbleEnd),
    "--theme-color-transcript-user-bubble-border": serializeColor(transcriptUserBubbleBorder),
    "--theme-color-transcript-user-code-bg": serializeColor(transcriptUserCodeBg),
    "--theme-color-transcript-user-code-border": serializeColor(transcriptUserCodeBorder),
  }
}

/** Returns the current system color-scheme choice observed by the browser runtime. */
export function readSystemThemeName() {
  return window.matchMedia(SYSTEM_COLOR_SCHEME_MEDIA_QUERY).matches ? "dark" : "light"
}

/** Returns the CSS media query used to follow the operating system color scheme. */
export function getSystemThemeMediaQuery() {
  return SYSTEM_COLOR_SCHEME_MEDIA_QUERY
}

/** Builds one resolved document-theme payload from the current appearance snapshot. */
export function buildAppearanceDocumentState(
  snapshot: AppearanceSnapshot,
): AppearanceDocumentState {
  const themeName = snapshot.mode === "system" ? snapshot.systemTheme : snapshot.mode

  return {
    themeName,
    attributes: {
      "data-theme": themeName,
      "data-theme-mode": snapshot.mode,
      "data-contrast": snapshot.highContrast ? "high" : "normal",
    },
    variables: deriveThemeVariables(themeName, snapshot.highContrast),
  }
}

/** Applies the resolved appearance payload to the root document element in place. */
export function applyAppearanceSnapshot(snapshot: AppearanceSnapshot) {
  const root = document.documentElement
  const documentState = buildAppearanceDocumentState(snapshot)

  for (const [name, value] of Object.entries(documentState.attributes)) {
    root.setAttribute(name, value)
  }

  root.style.colorScheme = documentState.themeName

  for (const [name, value] of Object.entries(documentState.variables)) {
    root.style.setProperty(name, value)
  }
}
