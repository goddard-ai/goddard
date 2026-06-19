/** Allows browser entrypoints to import built CSS assets. */
declare module "*.css"

/** Allows Lingui catalogs to be loaded through Vite. */
declare module "*.po" {
  import type { Messages } from "@lingui/core"

  export const messages: Messages
}

/** Allows importing SVG assets as raw markup strings. */
declare module "*.svg?raw" {
  const content: string
  export default content
}

/** Electrobun re-exports Three.js without types. This fixes type-checking errors. */
declare module "three"
