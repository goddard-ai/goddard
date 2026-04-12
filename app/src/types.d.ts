/** Allows browser entrypoints to import built CSS assets. */
declare module "*.css"

/** Allows importing SVG assets as raw markup strings. */
declare module "*.svg?raw" {
  const content: string
  export default content
}

/** Electrobun re-exports Three.js without types. This fixes type-checking errors. */
declare module "three"

/** Ark UI currently pulls source-typed transitive deps that `tsgo` rejects under this app config. */
declare module "@ark-ui/react/dialog" {
  export const Dialog: any
}

/** Ark UI currently pulls source-typed transitive deps that `tsgo` rejects under this app config. */
declare module "@ark-ui/react/portal" {
  export const Portal: any
}

/** Ark UI currently pulls source-typed transitive deps that `tsgo` rejects under this app config. */
declare module "@ark-ui/react/tooltip" {
  export const Tooltip: any
}
