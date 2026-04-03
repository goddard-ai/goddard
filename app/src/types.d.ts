/** Allows browser entrypoints to import built CSS assets. */
declare module "*.css"

/** Allows importing SVG assets as raw markup strings. */
declare module "*.svg?raw" {
  const content: import("~/components/SvgIcon").SvgIconData
  export default content
}

/** Electrobun re-exports Three.js without types. This fixes type-checking errors. */
declare module "three"
