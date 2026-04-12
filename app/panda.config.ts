import { defineConfig } from "@pandacss/dev"

export default defineConfig({
  preflight: true,
  presets: [],
  include: ["./src/**/*.{js,jsx,ts,tsx}"],
  exclude: [],
  outdir: "styled-system",
  jsxFramework: "preact",
  theme: {
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
        transcript: {
          glow: { value: "var(--theme-color-transcript-glow)" },
          glowEdge: { value: "var(--theme-color-transcript-glow-edge)" },
          userBubble: {
            start: { value: "var(--theme-color-transcript-user-bubble-start)" },
            end: { value: "var(--theme-color-transcript-user-bubble-end)" },
            border: { value: "var(--theme-color-transcript-user-bubble-border)" },
          },
          userCode: {
            bg: { value: "var(--theme-color-transcript-user-code-bg)" },
            border: { value: "var(--theme-color-transcript-user-code-border)" },
          },
        },
      },
    },
  },
})
