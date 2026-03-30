import { defineConfig } from "@pandacss/dev"

/** Minimal Panda CSS setup with a white canvas and restrained bluish-gray accents by default. */
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
        background: { value: "#ffffff" },
        surface: { value: "#f5f7fa" },
        panel: { value: "#fbfcfe" },
        border: { value: "#d8e0ea" },
        text: { value: "#18212b" },
        muted: { value: "#66758a" },
        accent: { value: "#8fa2b8" },
        accentStrong: { value: "#6f8298" },
        danger: { value: "#d45d5d" },
      },
    },
  },
})
