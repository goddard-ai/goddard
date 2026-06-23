import { metadata as rouzerMetadata, type RouteMetadata } from "rouzer"

/**
 * Attaches Rouzer route metadata without leaking Rouzer's private marker symbol
 * into exported IPC route declaration types.
 */
export function metadata(value: RouteMetadata) {
  return rouzerMetadata(value) as object
}
