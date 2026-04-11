import { defineConfig } from "@pandacss/dev"

export default defineConfig({
  preflight: true,
  presets: [],
  include: ["./src/**/*.{js,jsx,ts,tsx}"],
  exclude: [],
  outdir: "styled-system",
  jsxFramework: "preact",
  theme: {
    tokens: {
      colors: {
        seed: {
          accent: { value: "#8fa2b8" },
          background: { value: "#ffffff" },
          foreground: { value: "#18212b" },
        },
      },
    },
    semanticTokens: {
      colors: {
        background: { value: "var(--theme-color-background)" },
        surface: { value: "var(--theme-color-surface)" },
        panel: { value: "var(--theme-color-panel)" },
        border: { value: "var(--theme-color-border)" },
        text: { value: "var(--theme-color-text)" },
        muted: { value: "var(--theme-color-muted)" },
        accent: { value: "var(--theme-color-accent)" },
        accentStrong: { value: "var(--theme-color-accent-strong)" },
        accentFg: { value: "var(--theme-color-accent-fg)" },
        danger: { value: "var(--theme-color-danger)" },
        overlay: { value: "var(--theme-color-overlay)" },
        shadow: { value: "var(--theme-color-shadow)" },
        bg: {
          canvas: { value: "var(--theme-color-background)" },
          surface: { value: "var(--theme-color-surface)" },
          panel: { value: "var(--theme-color-panel)" },
        },
        fg: {
          default: { value: "var(--theme-color-text)" },
          muted: { value: "var(--theme-color-muted)" },
          onAccent: { value: "var(--theme-color-accent-fg)" },
        },
        feedback: {
          danger: { value: "var(--theme-color-danger)" },
        },
      },
    },
  },
})
