import { defineConfig } from "@pandacss/dev"

/** Minimal Panda CSS setup for the app with custom color tokens only. */
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
        background: { value: "#0d1117" },
        surface: { value: "#161b22" },
        border: { value: "#30363d" },
        text: { value: "#c9d1d9" },
        muted: { value: "#8b949e" },
        accent: { value: "#58a6ff" },
        danger: { value: "#f85149" },
      },
    },
  },
})
